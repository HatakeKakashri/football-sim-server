//! The roster a match publishes and the frames name their players by.
//!
//! The roster holds the match's 22 players once: one row per fixture
//! player, home rows first then away rows, each in the fixture's roster
//! order. The rows are what the manifest publishes and what a frame
//! names its players by, so the ids live here instead of in every
//! frame.

use std::collections::HashMap;

use sim_components::{Player, PlayerIdentity, TeamId};
use sim_core::Simulation;

use crate::api::model::{RosterEntry, RosterIndex, SIDE_AWAY, SIDE_HOME};

/// Roster published in the manifest: one row per fixture player, in the
/// fixture's roster order, plus the entity → roster row map the frames
/// use.
pub struct Roster {
    /// The manifest's roster rows: home rows first, then away rows.
    pub entries: Vec<RosterEntry>,
    /// The roster row of every player entity, by entity bits.
    pub rows: HashMap<u64, RosterIndex>,
}

impl Roster {
    /// The fixture id of the player a roster row names.
    ///
    /// The id is borrowed from the roster, never copied: the roster
    /// holds the 22 ids of a match once.
    pub fn id_of(&self, row: RosterIndex) -> &str {
        &row.row(&self.entries).id
    }
}

/// Build the roster from the fixture.
///
/// The rows are the fixture's own players — their `player_id`, name and
/// squad number — in the fixture's roster order, and each row's
/// position is the formation slot the player occupies (the fixture's
/// formation table at the player's roster index).
///
/// The entity → row map comes from the world's [`PlayerIdentity`]
/// components, so a frame names the same player the manifest lists:
/// the row of a player is the row that carries its fixture id on its
/// own side. A player the roster does not know is left out of the map,
/// and the capture refuses to name it.
pub fn build_roster(sim: &Simulation, fixture: &crate::api::MatchFixture) -> Roster {
    let mut entries = side_roster(
        SIDE_HOME,
        &fixture.home_team,
        &fixture.validated_match.home_setup,
    );
    entries.extend(side_roster(
        SIDE_AWAY,
        &fixture.away_team,
        &fixture.validated_match.away_setup,
    ));

    let mut rows = HashMap::with_capacity(entries.len());
    for entity in sim.world().iter_entities() {
        let (Some(identity), Some(player)) =
            (entity.get::<PlayerIdentity>(), entity.get::<Player>())
        else {
            continue;
        };
        let side = side_of(player.team_id);
        let Some(row) = entries
            .iter()
            .position(|entry| entry.team == side && entry.id == identity.id)
        else {
            // A player the roster does not know: the capture refuses to
            // name it rather than inventing a row for it.
            continue;
        };
        rows.insert(
            entity.id().to_bits(),
            RosterIndex::new(u8::try_from(row).expect("invariant: a squad has 11 players")),
        );
    }
    Roster { entries, rows }
}

/// The side a team id plays for.
fn side_of(team_id: TeamId) -> &'static str {
    if team_id == TeamId(0) {
        SIDE_HOME
    } else {
        SIDE_AWAY
    }
}

/// One side's roster rows: the fixture's players in roster order.
fn side_roster(
    side: &str,
    team: &sim_components::fixture::ValidatedTeam,
    setup: &sim_components::fixture::ValidatedSide,
) -> Vec<RosterEntry> {
    let slots = setup.formation.table().slots;
    team.players
        .iter()
        .enumerate()
        .map(|(index, player)| RosterEntry {
            id: player.id.clone(),
            team: side.to_string(),
            name: player.name.clone(),
            number: player.number,
            position: slots
                .get(index)
                .map_or_else(|| "UNK".to_string(), |slot| slot.position.to_string()),
        })
        .collect()
}
