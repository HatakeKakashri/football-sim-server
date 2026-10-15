//! Filesystem loading of TOML fixtures at server startup (PRD §5).
//!
//! Reads every file in `<fixtures_dir>/teams/` and
//! `<fixtures_dir>/matches/`, parses TOML, validates every PRD §5.3 rule,
//! and enforces cross-fixture invariants (`player_id` and `match_id`
//! uniqueness, referenced teams exist). A validation failure aborts boot
//! with an error naming the file, the rule, and the offending value.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use sim_components::fixture::{
    validate_match, validate_team, FixtureError, MatchRaw, TeamRaw, ValidatedMatch, ValidatedTeam,
};

/// Default fixtures directory (PRD config: `fixtures_dir = "./fixtures"`).
pub const DEFAULT_FIXTURES_DIR: &str = "./fixtures";

/// The complete, validated fixture set: every team and match fixture.
#[derive(Debug, Clone, Default)]
pub struct FixtureSet {
    /// Teams by fixture id.
    pub teams: HashMap<String, ValidatedTeam>,
    /// Matches by fixture id.
    pub matches: HashMap<String, ValidatedMatch>,
}

/// Load and validate every fixture file in `fixtures_dir`.
///
/// A missing or empty fixtures directory is not an error (it yields an
/// empty fixture set); every file that *is* present must fully validate.
/// Any failure returns a [`FixtureError`] naming the file, the violated
/// rule, and the offending value.
///
/// # Errors
///
/// Returns a [`FixtureError`] on I/O failure, TOML parse failure, or any
/// violated §5.3 rule.
pub fn load(fixtures_dir: &Path) -> Result<FixtureSet, FixtureError> {
    let mut teams: HashMap<String, ValidatedTeam> = HashMap::new();
    let mut matches: HashMap<String, ValidatedMatch> = HashMap::new();
    // player_id -> file where it was first declared (cross-fixture uniqueness).
    let mut player_ids: HashMap<String, PathBuf> = HashMap::new();

    let teams_dir = fixtures_dir.join("teams");
    if teams_dir.is_dir() {
        for path in sorted_toml_files(&teams_dir)? {
            let raw: TeamRaw = parse_file(&path)?;
            let team = validate_team(&path, &raw)?;
            for player in &team.players {
                if let Some(first_seen_in) = player_ids.insert(player.id.clone(), path.clone()) {
                    return Err(FixtureError::DuplicatePlayerId {
                        file: path,
                        player_id: player.id.clone(),
                        first_seen_in,
                    });
                }
            }
            teams.insert(team.id.clone(), team);
        }
    }

    let matches_dir = fixtures_dir.join("matches");
    // match_id -> file where it was first declared (cross-fixture uniqueness).
    let mut match_ids: HashMap<String, PathBuf> = HashMap::new();
    if matches_dir.is_dir() {
        for path in sorted_toml_files(&matches_dir)? {
            let raw: MatchRaw = parse_file(&path)?;
            let match_id = raw.match_.id.clone();
            if let Some(first_seen_in) = match_ids.insert(match_id.clone(), path.clone()) {
                return Err(FixtureError::DuplicateMatchId {
                    file: path,
                    match_id,
                    first_seen_in,
                });
            }
            let mat = validate_match(&path, &raw, &teams)?;
            matches.insert(mat.id.clone(), mat);
        }
    }

    Ok(FixtureSet { teams, matches })
}

/// Load and validate the default fixtures directory (`./fixtures`).
///
/// # Errors
///
/// Propagates any [`FixtureError`] returned by [`load`].
pub fn load_default() -> Result<FixtureSet, FixtureError> {
    load(Path::new(DEFAULT_FIXTURES_DIR))
}

/// Read and TOML-parse one fixture file.
///
/// # Errors
///
/// Returns a [`FixtureError::Io`] when the file cannot be read, or a
/// [`FixtureError::Toml`] when the contents are not valid TOML.
fn parse_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, FixtureError> {
    let contents =
        fs::read_to_string(path).map_err(|source| FixtureError::Io {
            file: path.to_path_buf(),
            source: source.to_string(),
        })?;
    toml::from_str(&contents).map_err(|source| FixtureError::Toml {
        file: path.to_path_buf(),
        source: source.to_string(),
    })
}

/// All `*.toml` files in `dir`, sorted by file name for deterministic order.
fn sorted_toml_files(dir: &Path) -> Result<Vec<PathBuf>, FixtureError> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|source| FixtureError::Io {
            file: dir.to_path_buf(),
            source: source.to_string(),
        })?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension() == Some(std::ffi::OsStr::new("toml")))
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    use sim_components::ProfileKind;

    /// Write a TOML string to `<root>/<name>` and return its path.
    fn temp_toml(root: &Path, name: &str, contents: &str) -> PathBuf {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut file = std::fs::File::create(&path).expect("temp fixture file");
        file.write_all(contents.as_bytes()).expect("temp fixture write");
        path
    }

    #[allow(
        clippy::too_many_lines,
        reason = "fixture helper is a single inline TOML literal; splitting it across helpers would hurt readability without improving correctness"
    )]
    fn valid_team_toml() -> String {
        r#"
[team]
id = "t-alpha"
name = "Alpha FC"

[[players]]
id = "p-alpha-gk"
name = "G. Keeper"
number = 1
kind = "goalkeeper"
trained_positions = ["GK"]
[players.attrs]
reflexes = 110
anticipation = 105
rushing_out = 90
throwing = 100
kicking = 95
punching = 85
aerial_reach = 105
communication = 100
agility = 95
concentration = 90
fitness = 80
strength = 85
aggression = 70
speed = 60
creativity = 50

[[players]]
id = "p-alpha-dl"
name = "D. Left"
number = 2
kind = "outfield"
trained_positions = ["DL"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-dc1"
name = "D. Centre 1"
number = 3
kind = "outfield"
trained_positions = ["DC"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-dc2"
name = "D. Centre 2"
number = 4
kind = "outfield"
trained_positions = ["DC"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-dr"
name = "D. Right"
number = 5
kind = "outfield"
trained_positions = ["DR"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-ml"
name = "M. Left"
number = 6
kind = "outfield"
trained_positions = ["ML"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-mc1"
name = "M. Centre 1"
number = 7
kind = "outfield"
trained_positions = ["MC"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-mc2"
name = "M. Centre 2"
number = 8
kind = "outfield"
trained_positions = ["MC"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-mr"
name = "M. Right"
number = 9
kind = "outfield"
trained_positions = ["MR"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-st1"
name = "S. Triker 1"
number = 10
kind = "outfield"
trained_positions = ["ST"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55

[[players]]
id = "p-alpha-st2"
name = "S. Triker 2"
number = 11
kind = "outfield"
trained_positions = ["ST"]
[players.attrs]
tackling = 110
marking = 105
positioning = 115
heading = 110
bravery = 100
passing = 80
dribbling = 60
crossing = 40
shooting = 45
finishing = 40
fitness = 95
strength = 110
aggression = 90
speed = 75
creativity = 55
"#
        .to_string()
    }

    fn valid_match_toml() -> String {
        r#"
[match]
id = "m-0001"
seed = 42
home = "t-alpha"
away = "t-bravo"

[match.home_setup]
formation = "4-4-2"
mentality = "balance"
tactic = "possession"

[match.away_setup]
formation = "4-4-2"
mentality = "attack"
tactic = "counter_attack"
"#
        .to_string()
    }

    #[test]
    fn test_missing_fixtures_dir_is_empty_set() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_missing");
        let _ = std::fs::remove_dir_all(&root);
        let set = load(&root).expect("missing dir yields empty set");
        assert!(set.teams.is_empty());
        assert!(set.matches.is_empty());
    }

    #[test]
    fn test_empty_fixtures_dir_is_empty_set() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_empty");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        std::fs::create_dir_all(root.join("matches")).ok();
        let set = load(&root).expect("empty dir yields empty set");
        assert!(set.teams.is_empty());
        assert!(set.matches.is_empty());
    }

    #[test]
    fn test_valid_fixtures_load() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_valid");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        std::fs::create_dir_all(root.join("matches")).ok();
        // t-bravo is a copy of t-alpha with different ids so the match
        // references two existing teams.
        let t_bravo = valid_team_toml().replace("t-alpha", "t-bravo").replace("p-alpha-", "p-bravo-");
        temp_toml(&root, "teams/t-alpha.toml", &valid_team_toml());
        temp_toml(&root, "teams/t-bravo.toml", &t_bravo);
        temp_toml(&root, "matches/m-0001.toml", &valid_match_toml());
        let set = load(&root).expect("valid fixtures should load");
        assert_eq!(set.teams.len(), 2);
        assert_eq!(set.matches.len(), 1);
        assert!(set.matches.contains_key("m-0001"));
        let home = set.teams.get("t-alpha").expect("home team");
        assert_eq!(home.players.len(), 11);
        assert_eq!(
            home.players.iter().filter(|p| p.kind == ProfileKind::Goalkeeper).count(),
            1
        );
    }

    #[test]
    fn test_duplicate_player_id_across_teams_aborts() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_dup_player");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        let mut team_b = valid_team_toml().replace("t-alpha", "t-bravo");
        // Reuse t-alpha's goalkeeper id in t-bravo.
        team_b = team_b.replace("p-bravo-gk", "p-alpha-gk");
        temp_toml(&root, "teams/t-alpha.toml", &valid_team_toml());
        temp_toml(&root, "teams/t-bravo.toml", &team_b);
        let error = load(&root).unwrap_err();
        assert_eq!(error.rule(), "player_id unique across all fixtures");
        assert!(error.to_string().contains("p-alpha-gk"));
    }

    #[test]
    fn test_duplicate_match_id_aborts() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_dup_match");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        std::fs::create_dir_all(root.join("matches")).ok();
        temp_toml(&root, "teams/t-alpha.toml", &valid_team_toml());
        temp_toml(&root, "teams/t-bravo.toml", &valid_team_toml().replace("t-alpha", "t-bravo").replace("p-alpha-", "p-bravo-"));
        temp_toml(&root, "matches/m-0001.toml", &valid_match_toml());
        temp_toml(&root, "matches/m-0002.toml", &valid_match_toml());
        let error = load(&root).unwrap_err();
        assert_eq!(error.rule(), "match_id unique");
    }

    #[test]
    fn test_referenced_team_missing_aborts() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_missing_team");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        std::fs::create_dir_all(root.join("matches")).ok();
        temp_toml(&root, "teams/t-alpha.toml", &valid_team_toml());
        // m-0001 references t-bravo which has no fixture.
        temp_toml(&root, "matches/m-0001.toml", &valid_match_toml());
        let error = load(&root).unwrap_err();
        assert_eq!(error.rule(), "referenced teams exist");
        assert!(error.to_string().contains("unknown team t-bravo"));
    }

    #[test]
    fn test_attribute_out_of_range_aborts() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_attr_range");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        std::fs::create_dir_all(root.join("matches")).ok();
        let mut team_toml = valid_team_toml();
        // Insert a second goalkeeper? No: change one attribute to 0.
        team_toml = team_toml.replace("tackling = 110", "tackling = 0", );
        temp_toml(&root, "teams/t-alpha.toml", &team_toml);
        temp_toml(&root, "teams/t-bravo.toml", &valid_team_toml().replace("t-alpha", "t-bravo"));
        temp_toml(&root, "matches/m-0001.toml", &valid_match_toml());
        let error = load(&root).unwrap_err();
        assert_eq!(error.rule(), "every attribute is an integer in 1..=200");
        assert!(error.to_string().contains("tackling = 0"));
    }

    #[test]
    fn test_seed_missing_aborts() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_seed");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        std::fs::create_dir_all(root.join("matches")).ok();
        temp_toml(&root, "teams/t-alpha.toml", &valid_team_toml());
        temp_toml(&root, "teams/t-bravo.toml", &valid_team_toml().replace("t-alpha", "t-bravo").replace("p-alpha-", "p-bravo-"));
        let mut match_toml = valid_match_toml();
        match_toml = match_toml.replace("seed = 42", "");
        temp_toml(&root, "matches/m-0001.toml", &match_toml);
        let error = load(&root).unwrap_err();
        assert_eq!(error.rule(), "seed present");
    }

    #[test]
    fn test_invalid_toml_aborts() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_toml");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        temp_toml(&root, "teams/broken.toml", "id = \"x\"\nnot valid toml [[[\n");
        let error = load(&root).unwrap_err();
        assert_eq!(error.rule(), "fixture is not valid TOML");
    }

    #[test]
    fn test_error_message_includes_file_rule_and_value() {
        let root = std::env::temp_dir().join("football_sim_fixture_test_display");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("teams")).ok();
        std::fs::create_dir_all(root.join("matches")).ok();
        let mut team_toml = valid_team_toml();
        team_toml = team_toml.replace("tackling = 110", "tackling = 201");
        temp_toml(&root, "teams/t-alpha.toml", &team_toml);
        temp_toml(&root, "teams/t-bravo.toml", &valid_team_toml().replace("t-alpha", "t-bravo"));
        temp_toml(&root, "matches/m-0001.toml", &valid_match_toml());
        let error = load(&root).unwrap_err();
        let message = error.to_string();
        assert!(message.starts_with(&format!("{}", root.join("teams/t-alpha.toml").display())));
        assert!(message.contains("every attribute is an integer in 1..=200"));
        assert!(message.contains("tackling = 201"));
    }

}
