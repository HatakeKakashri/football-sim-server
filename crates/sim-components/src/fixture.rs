//! TOML fixtures (PRD §5) and startup validation (PRD §5.3).
//!
//! This module is pure: it describes the fixture shape, validates every
//! §5.3 rule, and reports precise errors naming the file, the rule, and
//! the offending value. Filesystem I/O and TOML parsing live in
//! `sim_server::fixtures`; this module has no file or TOML dependency.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use super::Attribute;
use super::Formation;
use super::Mentality;
use super::PlayerProfile;
use super::PosSlot;
use super::ProfileKind;
use super::Tactic;

/// A precise fixture validation error.
///
/// Every variant carries the file it was found in, the §5.3 rule that was
/// violated, and the offending value, so boot can be aborted with a
/// message that points operators straight at the problem.
#[derive(Debug, Clone)]
pub enum FixtureError {
    Io {
        file: PathBuf,
        source: String,
    },
    Toml {
        file: PathBuf,
        source: String,
    },
    /// A team does not have exactly 11 players.
    NotExactlyElevenPlayers {
        file: PathBuf,
        team_id: String,
        count: usize,
    },
    /// A `player_id` appears in more than one fixture (or twice in one).
    DuplicatePlayerId {
        file: PathBuf,
        player_id: String,
        first_seen_in: PathBuf,
    },
    /// A `match_id` appears in more than one match fixture.
    DuplicateMatchId {
        file: PathBuf,
        match_id: String,
        first_seen_in: PathBuf,
    },
    /// The formation's slots cannot each be filled exactly once by the
    /// team's players respecting trained positions.
    FormationSlotsNotFilledExactlyOnce {
        file: PathBuf,
        match_id: String,
        side: String,
        team_id: String,
        formation: String,
    },
    /// The goalkeeper count is not exactly one (the GK slot must hold a
    /// goalkeeper, and no goalkeeper may appear outside it).
    GkCountNotOne {
        file: PathBuf,
        team_id: String,
        count: usize,
    },
    /// An attribute value is outside `1..=200`.
    AttributeOutOfRange {
        file: PathBuf,
        player_id: String,
        attribute: String,
        value: i64,
    },
    /// An attribute key is not a known attribute for the player's kind.
    UnknownAttribute {
        file: PathBuf,
        player_id: String,
        attribute: String,
    },
    /// A required attribute for the player's kind is missing.
    MissingAttribute {
        file: PathBuf,
        player_id: String,
        attribute: String,
    },
    /// A player does not have 1-3 trained positions.
    TrainedPositionsCountInvalid {
        file: PathBuf,
        player_id: String,
        count: usize,
    },
    /// A trained position is not valid for the player's kind.
    TrainedPositionInvalidForKind {
        file: PathBuf,
        player_id: String,
        position: PosSlot,
    },
    /// A player's `kind` is not `outfield` or `goalkeeper`.
    UnknownPlayerKind {
        file: PathBuf,
        player_id: String,
        kind: String,
    },
    /// A match fixture has no `seed`.
    SeedMissing {
        file: PathBuf,
        match_id: String,
    },
    /// A match fixture references a team that has no fixture.
    TeamNotFound {
        file: PathBuf,
        match_id: String,
        team_id: String,
    },
    /// A side's `formation` is not a registered formation name.
    UnknownFormation {
        file: PathBuf,
        match_id: String,
        side: String,
        formation: String,
    },
    /// A side's `mentality` is not a recognised mentality name.
    UnknownMentality {
        file: PathBuf,
        match_id: String,
        side: String,
        mentality: String,
    },
    /// A side's `tactic` is not a recognised tactic name.
    UnknownTactic {
        file: PathBuf,
        match_id: String,
        side: String,
        tactic: String,
    },
}

impl FixtureError {
    /// The §5.3 rule (or parse concern) this error reports.
    #[must_use]
    pub const fn rule(&self) -> &'static str {
        match self {
            Self::Io { .. } => "could not read fixture file",
            Self::Toml { .. } => "fixture is not valid TOML",
            Self::NotExactlyElevenPlayers { .. } => {
                "exactly 11 players per team"
            }
            Self::DuplicatePlayerId { .. } => {
                "player_id unique across all fixtures"
            }
            Self::DuplicateMatchId { .. } => "match_id unique",
            Self::FormationSlotsNotFilledExactlyOnce { .. } => {
                "formation slots each filled exactly once"
            }
            Self::GkCountNotOne { .. } => {
                "GK slot holds a goalkeeper and no other slot may"
            }
            Self::AttributeOutOfRange { .. }
            | Self::UnknownAttribute { .. }
            | Self::MissingAttribute { .. } => {
                "every attribute is an integer in 1..=200"
            }
            Self::TrainedPositionsCountInvalid { .. }
            | Self::TrainedPositionInvalidForKind { .. } => {
                "1-3 trained positions per player, valid for kind"
            }
            Self::UnknownPlayerKind { .. } => "player kind is outfield or goalkeeper",
            Self::SeedMissing { .. } => "seed present",
            Self::TeamNotFound { .. } => "referenced teams exist",
            Self::UnknownFormation { .. } => "formation is a registered formation name",
            Self::UnknownMentality { .. } => "mentality is a recognised mentality name",
            Self::UnknownTactic { .. } => "tactic is a recognised tactic name",
        }
    }

    /// A human description of the offending value.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::Io { source, .. } | Self::Toml { source, .. } => source.clone(),
            Self::NotExactlyElevenPlayers { count, .. } => {
                format!("team has {count} players, expected 11")
            }
            Self::DuplicatePlayerId { player_id, first_seen_in, .. } => {
                format!(
                    "player_id {player_id} already declared in {}",
                    first_seen_in.display()
                )
            }
            Self::DuplicateMatchId { match_id, first_seen_in, .. } => {
                format!(
                    "match_id {match_id} already declared in {}",
                    first_seen_in.display()
                )
            }
            Self::FormationSlotsNotFilledExactlyOnce {
                match_id,
                side,
                team_id,
                formation,
                ..
            } => {
                format!(
                    "match {match_id} {side} side team {team_id} with formation {formation} \
                     cannot fill every formation slot exactly once from trained positions"
                )
            }
            Self::GkCountNotOne { count, .. } => {
                format!("team has {count} goalkeepers, exactly 1 required")
            }
            Self::AttributeOutOfRange { player_id, attribute, value, .. } => {
                format!("player {player_id} attribute {attribute} = {value} (out of range 1..=200)")
            }
            Self::UnknownAttribute { player_id, attribute, .. } => {
                format!("player {player_id} attribute {attribute} is not known for this kind")
            }
            Self::MissingAttribute { player_id, attribute, .. } => {
                format!("player {player_id} is missing required attribute {attribute}")
            }
            Self::TrainedPositionsCountInvalid { player_id, count, .. } => {
                format!("player {player_id} has {count} trained positions, expected 1-3")
            }
            Self::TrainedPositionInvalidForKind { player_id, position, .. } => {
                format!("player {player_id} trained position {position} is not valid for this kind")
            }
            Self::UnknownPlayerKind { player_id, kind, .. } => {
                format!("player {player_id} kind {kind} is not outfield or goalkeeper")
            }
            Self::SeedMissing { match_id, .. } => {
                format!("match {match_id} has no seed")
            }
            Self::TeamNotFound { match_id, team_id, .. } => {
                format!("match {match_id} references unknown team {team_id}")
            }
            Self::UnknownFormation { match_id, side, formation, .. } => {
                format!("match {match_id} {side} side formation {formation} is not registered")
            }
            Self::UnknownMentality { match_id, side, mentality, .. } => {
                format!("match {match_id} {side} side mentality {mentality} is not recognised")
            }
            Self::UnknownTactic { match_id, side, tactic, .. } => {
                format!("match {match_id} {side} side tactic {tactic} is not recognised")
            }
        }
    }
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.file().display(),
            self.rule(),
            self.detail()
        )
    }
}

impl std::error::Error for FixtureError {}

impl FixtureError {
    /// The file the error was found in.
    #[must_use]
    pub const fn file(&self) -> &PathBuf {
        match self {
            Self::Io { file, .. }
            | Self::Toml { file, .. }
            | Self::NotExactlyElevenPlayers { file, .. }
            | Self::DuplicatePlayerId { file, .. }
            | Self::DuplicateMatchId { file, .. }
            | Self::FormationSlotsNotFilledExactlyOnce { file, .. }
            | Self::GkCountNotOne { file, .. }
            | Self::AttributeOutOfRange { file, .. }
            | Self::UnknownAttribute { file, .. }
            | Self::MissingAttribute { file, .. }
            | Self::TrainedPositionsCountInvalid { file, .. }
            | Self::TrainedPositionInvalidForKind { file, .. }
            | Self::UnknownPlayerKind { file, .. }
            | Self::SeedMissing { file, .. }
            | Self::TeamNotFound { file, .. }
            | Self::UnknownFormation { file, .. }
            | Self::UnknownMentality { file, .. }
            | Self::UnknownTactic { file, .. } => file,
        }
    }
}

/// The deserialised shape of `fixtures/teams/<id>.toml` (PRD §5.4).
#[derive(Deserialize, Debug, Clone)]
pub struct TeamRaw {
    pub team: TeamHeader,
    pub players: Vec<PlayerRaw>,
}

/// The `[team]` header of a team fixture.
#[derive(Deserialize, Debug, Clone)]
pub struct TeamHeader {
    pub id: String,
    pub name: String,
}

/// One `[[players]]` entry of a team fixture.
#[derive(Deserialize, Debug, Clone)]
pub struct PlayerRaw {
    pub id: String,
    pub name: String,
    pub number: u8,
    pub kind: String,
    pub trained_positions: Vec<PosSlot>,
    #[serde(default)]
    pub attrs: HashMap<String, i64>,
}

/// The deserialised shape of `fixtures/matches/<id>.toml` (PRD §5.4).
///
/// TOML represents `[match]`, `[match.home_setup]` and `[match.away_setup]`
/// as subtables of one `match` table, so they deserialize into one nested
/// section (seracde merges dotted tables into a single struct).
#[derive(Deserialize, Debug, Clone)]
pub struct MatchRaw {
    #[serde(rename = "match")]
    pub match_: MatchSection,
}

/// The `[match]` table of a match fixture: the header plus both setups.
#[derive(Deserialize, Debug, Clone)]
pub struct MatchSection {
    pub id: String,
    pub seed: Option<u64>,
    pub home: String,
    pub away: String,
    #[serde(rename = "home_setup")]
    pub home_setup: SideRaw,
    #[serde(rename = "away_setup")]
    pub away_setup: SideRaw,
}

/// One `[match.*_setup]` block of a match fixture.
#[derive(Deserialize, Debug, Clone)]
pub struct SideRaw {
    pub formation: String,
    pub mentality: String,
    pub tactic: String,
}

/// A team that passed per-team §5.3 validation, ready to be used.
#[derive(Debug, Clone)]
pub struct ValidatedTeam {
    pub id: String,
    pub name: String,
    /// 11 players: exactly 1 goalkeeper (index tracked separately) and 10
    /// outfield players, each with a built `PlayerProfile`.
    pub players: Vec<ValidatedPlayer>,
}

/// A player that passed per-player §5.3 validation.
#[derive(Debug, Clone)]
pub struct ValidatedPlayer {
    pub id: String,
    pub name: String,
    pub number: u8,
    pub kind: ProfileKind,
    pub trained_positions: Vec<PosSlot>,
    pub profile: PlayerProfile,
}

/// A match side's setup that passed validation.
#[derive(Debug, Clone)]
pub struct ValidatedSide {
    pub formation: Formation,
    pub mentality: Mentality,
    pub tactic: Tactic,
}

/// A match that passed per-match §5.3 validation (team references exist,
/// formations/mentalities/tactics recognised, slots fillable).
#[derive(Debug, Clone)]
pub struct ValidatedMatch {
    pub id: String,
    pub seed: u64,
    pub home: String,
    pub away: String,
    pub home_setup: ValidatedSide,
    pub away_setup: ValidatedSide,
}

/// Map a fixture `kind` string to a [`ProfileKind`].
fn profile_kind_from_name(name: &str) -> Option<ProfileKind> {
    match name {
        "outfield" => Some(ProfileKind::Outfield),
        "goalkeeper" => Some(ProfileKind::Goalkeeper),
        _ => None,
    }
}

/// Positions valid for a kind (PRD §5.3: a player's trained positions must
/// be valid for the player's kind).
#[must_use]
pub const fn valid_positions_for(kind: ProfileKind) -> &'static [PosSlot] {
    match kind {
        ProfileKind::Outfield => &PosSlot::OUTFIELD,
        ProfileKind::Goalkeeper => &[PosSlot::GK],
    }
}

/// `true` when `position` is a valid trained position for `kind`.
#[must_use]
pub fn position_valid_for(kind: ProfileKind, position: PosSlot) -> bool {
    valid_positions_for(kind).contains(&position)
}

/// Map a fixture attribute name to the `Attribute` variant for a kind.
///
/// Physical & mental names (`fitness`, `strength`, `aggression`, `speed`,
/// `creativity`) are shared; the kind disambiguates the variant.
fn attribute_for(kind: ProfileKind, name: &str) -> Option<Attribute> {
    Attribute::ALL
        .iter()
        .copied()
        .find(|attribute| attribute.profile_kind() == kind && attribute.name() == name)
}

/// Validate one team fixture file against the per-team §5.3 rules.
///
/// # Errors
///
/// Returns a [`FixtureError`] for the first violated rule.
pub fn validate_team(file: &Path, raw: &TeamRaw) -> Result<ValidatedTeam, FixtureError> {
    // Rule: exactly 11 players per team.
    if raw.players.len() != 11 {
        return Err(FixtureError::NotExactlyElevenPlayers {
            file: file.to_path_buf(),
            team_id: raw.team.id.clone(),
            count: raw.players.len(),
        });
    }

    let mut players = Vec::with_capacity(11);
    let mut seen_player_ids: HashSet<String> = HashSet::new();
    for raw_player in &raw.players {
        // Rule: player_id unique (within this team too).
        if !seen_player_ids.insert(raw_player.id.clone()) {
            return Err(FixtureError::DuplicatePlayerId {
                file: file.to_path_buf(),
                player_id: raw_player.id.clone(),
                first_seen_in: file.to_path_buf(),
            });
        }
        players.push(validate_player(file, raw_player)?);
    }

    // Rule: the GK slot holds a goalkeeper and no other slot may → exactly
    // one goalkeeper, who fills the GK slot.
    let gk_count = players.iter().filter(|p| p.kind == ProfileKind::Goalkeeper).count();
    if gk_count != 1 {
        return Err(FixtureError::GkCountNotOne {
            file: file.to_path_buf(),
            team_id: raw.team.id.clone(),
            count: gk_count,
        });
    }

    Ok(ValidatedTeam {
        id: raw.team.id.clone(),
        name: raw.team.name.clone(),
        players,
    })
}

/// Validate one player entry against the per-player §5.3 rules.
fn validate_player(file: &Path, raw: &PlayerRaw) -> Result<ValidatedPlayer, FixtureError> {
    let kind = profile_kind_from_name(&raw.kind).ok_or_else(|| {
        FixtureError::UnknownPlayerKind {
            file: file.to_path_buf(),
            player_id: raw.id.clone(),
            kind: raw.kind.clone(),
        }
    })?;

    // Rule: 1-3 trained positions per player.
    if !(1..=3).contains(&raw.trained_positions.len()) {
        return Err(FixtureError::TrainedPositionsCountInvalid {
            file: file.to_path_buf(),
            player_id: raw.id.clone(),
            count: raw.trained_positions.len(),
        });
    }
    // Rule: trained positions valid for the player's kind.
    for &position in &raw.trained_positions {
        if !position_valid_for(kind, position) {
            return Err(FixtureError::TrainedPositionInvalidForKind {
                file: file.to_path_buf(),
                player_id: raw.id.clone(),
                position,
            });
        }
    }

    // Rule: every attribute is an integer in 1..=200, known for the kind,
    // and all attributes required by the kind are present.
    let mut profile = match kind {
        ProfileKind::Outfield => PlayerProfile::default_outfield(),
        ProfileKind::Goalkeeper => PlayerProfile::default_goalkeeper(),
    };
    let required: Vec<Attribute> = Attribute::ALL
        .iter()
        .filter(|attribute| attribute.profile_kind() == kind)
        .copied()
        .collect();
    let mut seen: HashSet<Attribute> = HashSet::new();
    for (name, &value) in &raw.attrs {
        let attribute = attribute_for(kind, name).ok_or_else(|| {
            FixtureError::UnknownAttribute {
                file: file.to_path_buf(),
                player_id: raw.id.clone(),
                attribute: name.clone(),
            }
        })?;
        if !(1..=200).contains(&value) {
            return Err(FixtureError::AttributeOutOfRange {
                file: file.to_path_buf(),
                player_id: raw.id.clone(),
                attribute: name.clone(),
                value,
            });
        }
        profile
            .set(attribute, u8::try_from(value).expect("invariant: attribute value is in 1..=200"))
            .expect("invariant: attribute in range and matches profile");
        seen.insert(attribute);
    }
    for attribute in &required {
        if !seen.contains(attribute) {
            return Err(FixtureError::MissingAttribute {
                file: file.to_path_buf(),
                player_id: raw.id.clone(),
                attribute: attribute.name().to_string(),
            });
        }
    }

    Ok(ValidatedPlayer {
        id: raw.id.clone(),
        name: raw.name.clone(),
        number: raw.number,
        kind,
        trained_positions: raw.trained_positions.clone(),
        profile,
    })
}

/// Validate one match fixture's side setup (formation/mentality/tactic and
/// that the referenced team can fill every formation slot exactly once).
fn validate_side(
    file: &Path,
    match_id: &str,
    side: &str,
    raw: &SideRaw,
    team: &ValidatedTeam,
) -> Result<ValidatedSide, FixtureError> {
    let formation = Formation::from_name(&raw.formation).ok_or_else(|| {
        FixtureError::UnknownFormation {
            file: file.to_path_buf(),
            match_id: match_id.to_string(),
            side: side.to_string(),
            formation: raw.formation.clone(),
        }
    })?;
    let mentality = Mentality::from_name(&raw.mentality).ok_or_else(|| {
        FixtureError::UnknownMentality {
            file: file.to_path_buf(),
            match_id: match_id.to_string(),
            side: side.to_string(),
            mentality: raw.mentality.clone(),
        }
    })?;
    let tactic = Tactic::from_name(&raw.tactic).ok_or_else(|| {
        FixtureError::UnknownTactic {
            file: file.to_path_buf(),
            match_id: match_id.to_string(),
            side: side.to_string(),
            tactic: raw.tactic.clone(),
        }
    })?;

    // Rule: formation slots each filled exactly once.
    fill_formation_slots(file, match_id, side, team, formation)?;

    Ok(ValidatedSide {
        formation,
        mentality,
        tactic,
    })
}

/// Validate one match fixture file against the per-match §5.3 rules.
///
/// `teams` holds every validated team, keyed by team id, so referenced
/// teams can be checked and formation slots filled.
///
/// # Panics
///
/// Panics if `teams` does not contain the match's home or away team; the
/// caller is expected to check team existence before calling.
///
/// # Errors
///
/// Returns a [`FixtureError`] for the first violated rule.
pub fn validate_match<S: std::hash::BuildHasher>(
    file: &Path,
    raw: &MatchRaw,
    teams: &HashMap<String, ValidatedTeam, S>,
) -> Result<ValidatedMatch, FixtureError> {
    // Rule: seed present.
    let seed = raw.match_.seed.ok_or_else(|| FixtureError::SeedMissing {
        file: file.to_path_buf(),
        match_id: raw.match_.id.clone(),
    })?;

    // Rule: referenced teams exist.
    for (_side, team_id) in [("home", &raw.match_.home), ("away", &raw.match_.away)] {
        if !teams.contains_key(team_id) {
            return Err(FixtureError::TeamNotFound {
                file: file.to_path_buf(),
                match_id: raw.match_.id.clone(),
                team_id: team_id.clone(),
            });
        }
    }

    let home_team = teams.get(&raw.match_.home).expect("invariant: home team exists");
    let away_team = teams.get(&raw.match_.away).expect("invariant: away team exists");
    let home_setup = validate_side(file, &raw.match_.id, "home", &raw.match_.home_setup, home_team)?;
    let away_setup = validate_side(file, &raw.match_.id, "away", &raw.match_.away_setup, away_team)?;

    Ok(ValidatedMatch {
        id: raw.match_.id.clone(),
        seed,
        home: raw.match_.home.clone(),
        away: raw.match_.away.clone(),
        home_setup,
        away_setup,
    })
}

/// Whether `team`'s players can fill every slot of `formation` exactly
/// once, respecting each player's trained positions.
///
/// The goalkeeper (exactly one, validated separately) fills the GK slot.
/// The 10 outfield players are matched to the 10 outfield slots so that
/// every slot is filled by exactly one player trained for that position.
fn fill_formation_slots(
    file: &Path,
    match_id: &str,
    side: &str,
    team: &ValidatedTeam,
    formation: Formation,
) -> Result<(), FixtureError> {
    let table = formation.table();
    // The GK slot is filled by the team's sole goalkeeper (validated
    // separately); every formation table has exactly one GK slot.
    table
        .slots
        .iter()
        .find(|slot| slot.position == PosSlot::GK)
        .expect("invariant: every formation table has exactly one GK slot");

    let outfield_slots: Vec<PosSlot> = table
        .slots
        .iter()
        .filter(|slot| slot.position != PosSlot::GK)
        .map(|slot| slot.position)
        .collect();
    let outfield_players: Vec<&ValidatedPlayer> = team
        .players
        .iter()
        .filter(|player| player.kind != ProfileKind::Goalkeeper)
        .collect();

    debug_assert_eq!(outfield_slots.len(), 10, "formation must have 10 outfield slots");
    debug_assert_eq!(
        outfield_players.len(),
        10,
        "team with 1 goalkeeper and 11 players must have 10 outfield players"
    );

    // Bipartite matching (Kuhn's algorithm): can every outfield slot be
    // assigned a distinct outfield player trained for that position?
    let adj: Vec<Vec<usize>> = outfield_slots
        .iter()
        .map(|position| {
            outfield_players
                .iter()
                .enumerate()
                .filter(|(_, player)| player.trained_positions.contains(position))
                .map(|(index, _)| index)
                .collect()
        })
        .collect();
    let mut player_to_slot: Vec<Option<usize>> = vec![None; outfield_players.len()];
    for slot_index in 0..outfield_slots.len() {
        let mut visited = vec![false; outfield_players.len()];
        if !assign_slot(slot_index, &adj, &mut player_to_slot, &mut visited) {
            return Err(FixtureError::FormationSlotsNotFilledExactlyOnce {
                file: file.to_path_buf(),
                match_id: match_id.to_string(),
                side: side.to_string(),
                team_id: team.id.clone(),
                formation: formation.name().to_string(),
            });
        }
    }
    Ok(())
}

/// Try to assign `slot` to a player via an augmenting path (DFS).
fn assign_slot(
    slot: usize,
    adj: &[Vec<usize>],
    player_to_slot: &mut Vec<Option<usize>>,
    visited: &mut [bool],
) -> bool {
    for &player in &adj[slot] {
        if visited[player] {
            continue;
        }
        visited[player] = true;
        if player_to_slot[player].is_none()
            || assign_slot(player_to_slot[player].unwrap(), adj, player_to_slot, visited)
        {
            player_to_slot[player] = Some(slot);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outfield_attrs() -> HashMap<String, i64> {
        HashMap::from([
            ("tackling".to_string(), 110),
            ("marking".to_string(), 105),
            ("positioning".to_string(), 115),
            ("heading".to_string(), 110),
            ("bravery".to_string(), 100),
            ("passing".to_string(), 80),
            ("dribbling".to_string(), 60),
            ("crossing".to_string(), 40),
            ("shooting".to_string(), 45),
            ("finishing".to_string(), 40),
            ("fitness".to_string(), 95),
            ("strength".to_string(), 110),
            ("aggression".to_string(), 90),
            ("speed".to_string(), 75),
            ("creativity".to_string(), 55),
        ])
    }

    fn gk_attrs() -> HashMap<String, i64> {
        HashMap::from([
            ("reflexes".to_string(), 110),
            ("anticipation".to_string(), 105),
            ("rushing_out".to_string(), 90),
            ("throwing".to_string(), 100),
            ("kicking".to_string(), 95),
            ("punching".to_string(), 85),
            ("aerial_reach".to_string(), 105),
            ("communication".to_string(), 100),
            ("agility".to_string(), 95),
            ("concentration".to_string(), 90),
            ("fitness".to_string(), 80),
            ("strength".to_string(), 85),
            ("aggression".to_string(), 70),
            ("speed".to_string(), 60),
            ("creativity".to_string(), 50),
        ])
    }

    /// A fully valid default team: 1 GK + 10 outfield players, each
    /// trained for a distinct 4-4-2 outfield slot, all attributes in range.
    fn valid_team_raw() -> TeamRaw {
        TeamRaw {
            team: TeamHeader {
                id: "t-alpha".to_string(),
                name: "Alpha FC".to_string(),
            },
            players: vec![
                // Goalkeeper: fills the GK slot.
                player("p-alpha-gk", "G. Keeper", 1, "goalkeeper", vec![PosSlot::GK], gk_attrs()),
                // Outfield players trained for each 4-4-2 outfield slot.
                player("p-alpha-dl", "D. Left", 2, "outfield", vec![PosSlot::DL], outfield_attrs()),
                player("p-alpha-dc1", "D. Centre 1", 3, "outfield", vec![PosSlot::DC], outfield_attrs()),
                player("p-alpha-dc2", "D. Centre 2", 4, "outfield", vec![PosSlot::DC], outfield_attrs()),
                player("p-alpha-dr", "D. Right", 5, "outfield", vec![PosSlot::DR], outfield_attrs()),
                player("p-alpha-ml", "M. Left", 6, "outfield", vec![PosSlot::ML], outfield_attrs()),
                player("p-alpha-mc1", "M. Centre 1", 7, "outfield", vec![PosSlot::MC], outfield_attrs()),
                player("p-alpha-mc2", "M. Centre 2", 8, "outfield", vec![PosSlot::MC], outfield_attrs()),
                player("p-alpha-mr", "M. Right", 9, "outfield", vec![PosSlot::MR], outfield_attrs()),
                player("p-alpha-st1", "S. Triker 1", 10, "outfield", vec![PosSlot::ST], outfield_attrs()),
                player("p-alpha-st2", "S. Triker 2", 11, "outfield", vec![PosSlot::ST], outfield_attrs()),
            ],
        }
    }

    fn player(
        id: &str,
        name: &str,
        number: u8,
        kind: &str,
        trained_positions: Vec<PosSlot>,
        attrs: HashMap<String, i64>,
    ) -> PlayerRaw {
        PlayerRaw {
            id: id.to_string(),
            name: name.to_string(),
            number,
            kind: kind.to_string(),
            trained_positions,
            attrs,
        }
    }

    fn file() -> PathBuf {
        PathBuf::from("fixtures/teams/t-alpha.toml")
    }

    #[test]
    fn test_valid_team_passes_all_rules() {
        let team = validate_team(&file(), &valid_team_raw()).expect("valid team should validate");
        assert_eq!(team.players.len(), 11);
        assert_eq!(
            team.players.iter().filter(|p| p.kind == ProfileKind::Goalkeeper).count(),
            1
        );
    }

    // ---- §5.3: exactly 11 players per team ----

    #[test]
    fn test_not_exactly_eleven_players_fails() {
        let mut raw = valid_team_raw();
        raw.players.pop(); // 10 players
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "exactly 11 players per team");
        assert!(error.detail().contains("10 players"));
        assert_eq!(error.file(), &file());
    }

    #[test]
    fn test_too_many_players_fails() {
        let mut raw = valid_team_raw();
        raw.players.push(player(
            "p-alpha-extra",
            "Extra",
            12,
            "outfield",
            vec![PosSlot::ST],
            outfield_attrs(),
        )); // 12 players
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "exactly 11 players per team");
        assert!(error.detail().contains("12 players"));
    }

    // ---- §5.3: player_id unique across all fixtures ----

    #[test]
    fn test_duplicate_player_id_within_team_fails() {
        let mut raw = valid_team_raw();
        raw.players[1].id = raw.players[0].id.clone(); // duplicate within one team
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "player_id unique across all fixtures");
        assert!(error.detail().contains(&raw.players[0].id));
    }

    // Cross-team `player_id` uniqueness is enforced by the loader
    // (`sim_server::fixtures::load`) once every team fixture is known; it
    // is tested there.

    // ---- §5.3: every attribute is an integer in 1..=200 ----

    #[test]
    fn test_attribute_out_of_range_low_fails() {
        let mut raw = valid_team_raw();
        raw.players[1].attrs.insert("tackling".to_string(), 0);
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "every attribute is an integer in 1..=200");
        assert!(error.detail().contains("tackling = 0"));
    }

    #[test]
    fn test_attribute_out_of_range_high_fails() {
        let mut raw = valid_team_raw();
        raw.players[1].attrs.insert("shooting".to_string(), 201);
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "every attribute is an integer in 1..=200");
        assert!(error.detail().contains("shooting = 201"));
    }

    #[test]
    fn test_unknown_attribute_fails() {
        let mut raw = valid_team_raw();
        raw.players[1]
            .attrs
            .insert("not_a_real_attribute".to_string(), 100);
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "every attribute is an integer in 1..=200");
        assert!(error.detail().contains("not_a_real_attribute"));
    }

    #[test]
    fn test_missing_attribute_fails() {
        let mut raw = valid_team_raw();
        raw.players[1].attrs.remove("finishing");
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "every attribute is an integer in 1..=200");
        assert!(error.detail().contains("missing required attribute finishing"));
    }

    // ---- §5.3: 1-3 trained positions per player, valid for kind ----

    #[test]
    fn test_trained_positions_count_zero_fails() {
        let mut raw = valid_team_raw();
        raw.players[1].trained_positions.clear();
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "1-3 trained positions per player, valid for kind");
        assert!(error.detail().contains("0 trained positions"));
    }

    #[test]
    fn test_trained_positions_count_four_fails() {
        let mut raw = valid_team_raw();
        raw.players[1].trained_positions =
            vec![PosSlot::DL, PosSlot::DC, PosSlot::ML, PosSlot::MC];
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "1-3 trained positions per player, valid for kind");
        assert!(error.detail().contains("4 trained positions"));
    }

    #[test]
    fn test_trained_position_invalid_for_goalkeeper_kind_fails() {
        let mut raw = valid_team_raw();
        // The goalkeeper is trained as a defender: not valid for a goalkeeper.
        raw.players[0].trained_positions = vec![PosSlot::DC];
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "1-3 trained positions per player, valid for kind");
        assert!(error.detail().contains("DC"));
    }

    #[test]
    fn test_unknown_player_kind_fails() {
        let mut raw = valid_team_raw();
        raw.players[1].kind = "defender".to_string();
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "player kind is outfield or goalkeeper");
        assert!(error.detail().contains("defender"));
    }

    // ---- §5.3: GK slot holds a goalkeeper and no other slot may ----

    #[test]
    fn test_no_goalkeeper_fails_gk_rule() {
        let mut raw = valid_team_raw();
        // Re-train the sole goalkeeper as an outfield left-back so every
        // player is individually valid, leaving zero goalkeepers.
        raw.players[0].kind = "outfield".to_string();
        raw.players[0].trained_positions = vec![PosSlot::DL];
        raw.players[0].attrs = outfield_attrs();
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "GK slot holds a goalkeeper and no other slot may");
        assert!(error.detail().contains("0 goalkeepers"));
    }

    #[test]
    fn test_two_goalkeepers_fails_gk_rule() {
        let mut raw = valid_team_raw();
        raw.players[1].kind = "goalkeeper".to_string(); // 2 goalkeepers
        raw.players[1].trained_positions = vec![PosSlot::GK];
        raw.players[1].attrs = gk_attrs();
        let error = validate_team(&file(), &raw).unwrap_err();
        assert_eq!(error.rule(), "GK slot holds a goalkeeper and no other slot may");
        assert!(error.detail().contains("2 goalkeepers"));
    }

    // ---- §5.3: formation slots each filled exactly once ----

    #[test]
    fn test_formation_slots_not_fillable_fails() {
        // Formation-slot filling is a match-level rule: it needs a team
        // plus a formation. All outfield players train only as ST, so the
        // defence/midfield slots of 4-4-2 cannot be filled exactly once.
        let mut raw = valid_team_raw();
        for player in &mut raw.players.iter_mut().skip(1) {
            player.trained_positions = vec![PosSlot::ST];
        }
        let team = validate_team(&file(), &raw).expect("squad is individually valid");
        let mut match_raw = valid_match_raw();
        match_raw.match_.home_setup.formation = "4-4-2".to_string();
        let teams = HashMap::from([
            ("t-alpha".to_string(), team),
            ("t-bravo".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
        ]);
        let error = validate_match(&match_file(), &match_raw, &teams).unwrap_err();
        assert_eq!(error.rule(), "formation slots each filled exactly once");
    }

    #[test]
    fn test_multi_trained_players_can_fill_slots_passes() {
        // Extra trained positions must not break slot filling: every
        // outfield player keeps their primary slot and gains MC as a
        // second option; 4-4-2 still fills exactly once.
        let mut raw = valid_team_raw();
        for player in &mut raw.players.iter_mut().skip(1) {
            if !player.trained_positions.contains(&PosSlot::MC) {
                player.trained_positions.push(PosSlot::MC);
            }
        }
        let team = validate_team(&file(), &raw).expect("squad is individually valid");
        let match_raw = valid_match_raw();
        let teams = HashMap::from([
            ("t-alpha".to_string(), team),
            ("t-bravo".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
        ]);
        let m = validate_match(&match_file(), &match_raw, &teams)
            .expect("flexible squad should fill 4-4-2");
        assert_eq!(m.home_setup.formation, Formation::FourFourTwo);
    }

    // ---- §5.3: seed present / referenced teams exist / match setup ----

    fn valid_match_raw() -> MatchRaw {
        MatchRaw {
            match_: MatchSection {
                id: "m-0001".to_string(),
                seed: Some(42),
                home: "t-alpha".to_string(),
                away: "t-bravo".to_string(),
                home_setup: SideRaw {
                    formation: "4-4-2".to_string(),
                    mentality: "balance".to_string(),
                    tactic: "possession".to_string(),
                },
                away_setup: SideRaw {
                    formation: "4-4-2".to_string(),
                    mentality: "attack".to_string(),
                    tactic: "counter_attack".to_string(),
                },
            },
        }
    }

    fn match_file() -> PathBuf {
        PathBuf::from("fixtures/matches/m-0001.toml")
    }

    #[test]
    fn test_seed_missing_fails() {
        let mut raw = valid_match_raw();
        raw.match_.seed = None;
        let teams = HashMap::from([
            ("t-alpha".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
        ]);
        let error = validate_match(&match_file(), &raw, &teams).unwrap_err();
        assert_eq!(error.rule(), "seed present");
        assert!(error.detail().contains("has no seed"));
    }

    #[test]
    fn test_referenced_team_not_found_fails() {
        let raw = valid_match_raw();
        let teams = HashMap::new(); // no teams: t-alpha missing
        let error = validate_match(&match_file(), &raw, &teams).unwrap_err();
        assert_eq!(error.rule(), "referenced teams exist");
        assert!(error.detail().contains("unknown team t-alpha"));
    }

    #[test]
    fn test_unknown_formation_fails() {
        let mut raw = valid_match_raw();
        raw.match_.home_setup.formation = "9-9-9".to_string();
        let teams = HashMap::from([
            ("t-alpha".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
            ("t-bravo".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
        ]);
        let error = validate_match(&match_file(), &raw, &teams).unwrap_err();
        assert_eq!(error.rule(), "formation is a registered formation name");
        assert!(error.detail().contains("9-9-9"));
    }

    #[test]
    fn test_unknown_mentality_fails() {
        let mut raw = valid_match_raw();
        raw.match_.home_setup.mentality = "all_out_attack".to_string();
        let teams = HashMap::from([
            ("t-alpha".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
            ("t-bravo".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
        ]);
        let error = validate_match(&match_file(), &raw, &teams).unwrap_err();
        assert_eq!(error.rule(), "mentality is a recognised mentality name");
    }

    #[test]
    fn test_unknown_tactic_fails() {
        let mut raw = valid_match_raw();
        raw.match_.away_setup.tactic = "tiki_taka".to_string();
        let teams = HashMap::from([
            ("t-alpha".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
            ("t-bravo".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
        ]);
        let error = validate_match(&match_file(), &raw, &teams).unwrap_err();
        assert_eq!(error.rule(), "tactic is a recognised tactic name");
    }

    #[test]
    fn test_valid_match_passes() {
        let raw = valid_match_raw();
        let teams = HashMap::from([
            ("t-alpha".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
            ("t-bravo".to_string(), validate_team(&file(), &valid_team_raw()).unwrap()),
        ]);
        let m = validate_match(&match_file(), &raw, &teams).expect("valid match should validate");
        assert_eq!(m.seed, 42);
        assert_eq!(m.home_setup.formation, Formation::FourFourTwo);
        assert_eq!(m.away_setup.formation, Formation::FourFourTwo);
    }

    // ---- Display includes file, rule, and offending value ----

    #[test]
    fn test_error_display_names_file_rule_and_value() {
        let mut raw = valid_team_raw();
        raw.players[1].attrs.insert("tackling".to_string(), 0);
        let error = validate_team(&file(), &raw).unwrap_err();
        let message = error.to_string();
        assert!(message.starts_with("fixtures/teams/t-alpha.toml:"));
        assert!(message.contains("every attribute is an integer in 1..=200"));
        assert!(message.contains("tackling = 0"));
    }
}
