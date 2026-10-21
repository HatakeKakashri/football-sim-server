//! Fixture registry: `match_id` → validated match + both rosters.
//!
//! The registry is the only source of known match ids: unknown ids
//! get `404` (PRD §10.1). It holds no runtime state — the store
//! tracks the lifecycle of the matches started from it.

use std::collections::HashMap;

/// One registered fixture: a `match_id` with its validated match data and
/// the two teams' player rosters.
#[derive(Debug, Clone)]
pub struct MatchFixture {
    /// Fixture match id from the URL path.
    pub match_id: String,
    /// The validated match (seed, home/away team IDs, setups).
    pub validated_match: sim_components::fixture::ValidatedMatch,
    /// The home team's validated players.
    pub home_team: sim_components::fixture::ValidatedTeam,
    /// The away team's validated players.
    pub away_team: sim_components::fixture::ValidatedTeam,
}

/// Fixture registry. Unknown `match_id`s get `404` (PRD §10.1).
#[derive(Debug, Clone, Default)]
pub struct MatchRegistry {
    /// Fixtures by `match_id`.
    pub fixtures: HashMap<String, MatchFixture>,
}

impl MatchRegistry {
    /// Registry with a single fixture (this slice serves one fixture).
    #[must_use]
    pub fn single(
        match_id: &str,
        validated_match: sim_components::fixture::ValidatedMatch,
        home_team: sim_components::fixture::ValidatedTeam,
        away_team: sim_components::fixture::ValidatedTeam,
    ) -> Self {
        let mut registry = Self::default();
        registry.insert(match_id, validated_match, home_team, away_team);
        registry
    }

    /// Test-only constructor: creates a registry with a single fixture using
    /// placeholder profiles (for tests that don't need real fixture data).
    ///
    /// # Panics
    ///
    /// Panics if a generated player number does not fit in `u8`,
    /// which cannot happen for the eleven generated players.
    #[cfg(test)]
    #[must_use]
    pub fn single_test(match_id: &str, seed: u64) -> Self {
        use sim_components::{
            fixture::{ValidatedMatch, ValidatedSide, ValidatedTeam, ValidatedPlayer},
            profile::ProfileKind,
            Formation, Mentality, Tactic, PosSlot, PlayerProfile,
        };
        let mut registry = Self::default();

        // Create minimal validated match with placeholder data
        let validated_match = ValidatedMatch {
            id: match_id.to_string(),
            seed,
            home: "test-home".to_string(),
            away: "test-away".to_string(),
            home_setup: ValidatedSide {
                formation: Formation::FourFourTwo,
                mentality: Mentality::Balance,
                tactic: Tactic::Possession,
            },
            away_setup: ValidatedSide {
                formation: Formation::FourFourTwo,
                mentality: Mentality::Balance,
                tactic: Tactic::Possession,
            },
        };

        // Create minimal validated teams with 11 placeholder players each
        let make_team = |id: &str, name: &str| ValidatedTeam {
            id: id.to_string(),
            name: name.to_string(),
            players: (0..11).map(|i| ValidatedPlayer {
                id: format!("{id}-p{i:02}"),
                name: format!("Player {i}"),
                number: u8::try_from(i + 1).expect("invariant: player number fits in u8"),
                kind: if i == 0 { ProfileKind::Goalkeeper } else { ProfileKind::Outfield },
                trained_positions: if i == 0 { vec![PosSlot::GK] } else { vec![PosSlot::MC] },
                profile: if i == 0 { PlayerProfile::default_goalkeeper() } else { PlayerProfile::default_outfield() },
            }).collect(),
        };

        let home_team = make_team("test-home", "Home");
        let away_team = make_team("test-away", "Away");

        registry.insert(match_id, validated_match, home_team, away_team);
        registry
    }

    /// Add (or replace) a fixture.
    pub fn insert(
        &mut self,
        match_id: &str,
        validated_match: sim_components::fixture::ValidatedMatch,
        home_team: sim_components::fixture::ValidatedTeam,
        away_team: sim_components::fixture::ValidatedTeam,
    ) {
        self.fixtures.insert(
            match_id.to_string(),
            MatchFixture {
                match_id: match_id.to_string(),
                validated_match,
                home_team,
                away_team,
            },
        );
    }

    /// Look up a fixture by `match_id`.
    #[must_use]
    pub fn get(&self, match_id: &str) -> Option<&MatchFixture> {
        self.fixtures.get(match_id)
    }
}
