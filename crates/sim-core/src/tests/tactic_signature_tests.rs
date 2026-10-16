//! Tactic/mentality A9 signature tests (PRD §13.1 A9, PRD §7.2).
//!
//! Each preset differs from the `Balance`/`Possession` baseline on its
//! signature statistic, measured over several seeds:
//!
//! - `Possession`: shortens pass length.
//! - `CounterAttack`: lengthens pass length.
//! - `ParkTheBus`: lowers the defensive line.
//! - `Attack` / `Defend`: raise / lower the defensive line.
//! - `HighPress`: widens the press trigger distance (the `DistanceToPress`
//!   consideration's logistic midpoint scales with
//!   `TacticParameters::press_trigger_distance`).
//!
//! These are multi-seed match runs and are `#[ignore]`d per PRD §14; run
//! them manually in release:
//!
//! ```text
//! cargo test --release -p sim-core -- --ignored tactic_signature --nocapture
//! ```

use sim_components::{Mentality, Role, RoleComponent, TeamId, TeamSetup, Tactic};
use sim_math::Vec2;

use crate::{EventLog, MatchEvent, Simulation};

/// Home team id; the home team attacks +x in the first half, so defender
/// x is a line-height proxy (higher = further up the pitch).
const HOME: TeamId = TeamId(0);

/// Partial-match observation window (ticks = 6 minutes of game time).
const OBSERVE_TICKS: u64 = 24_000;

/// Seeds used by every signature test (paired across setups).
const SEEDS: [u64; 6] = [1001, 1002, 1003, 1004, 1005, 1006];

/// Baseline: Balance mentality + Possession tactic (PRD §7.2 reference).
fn baseline() -> TeamSetup {
    TeamSetup {
        formation: sim_components::Formation::FourFourTwo,
        mentality: Mentality::Balance,
        tactic: Tactic::Possession,
    }
}

/// Build a per-side setup for a given mentality/tactic (4-4-2 formation).
fn setup(mentality: Mentality, tactic: Tactic) -> TeamSetup {
    TeamSetup {
        formation: sim_components::Formation::FourFourTwo,
        mentality,
        tactic,
    }
}

/// Roles that hold the defensive line; their x-position tracks the line
/// height because they stay back rather than chasing the ball upfield.
const DEFENDER_ROLES: [Role; 3] = [
    Role::CenterBack,
    Role::FullBack,
    Role::DefensiveMidfielder,
];

/// Aggregated metrics from one observed match.
struct MatchMetrics {
    /// Pass lengths (metres), one per resolved `pass` event.
    pass_lengths: Vec<f32>,
    /// Mean over ticks of the deepest home defender's x-position
    /// (line-height proxy; deeper defenders chase the ball least).
    line_mean_x: f32,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "match pass counts are far below 2^24, so the usize -> f32 cast is exact"
)]
fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

/// Run `ticks` of a match and aggregate pass lengths plus the per-tick
/// deepest home-defender x-position (the defensive line).
fn observe(seed: u64, home_setup: TeamSetup, away_setup: TeamSetup) -> MatchMetrics {
    let mut sim = Simulation::new_with_setup(seed, home_setup, away_setup);
    let mut pass_lengths: Vec<f32> = Vec::new();
    let mut line_sum = 0.0;

    for _ in 0..OBSERVE_TICKS {
        sim.tick();

        let world = sim.world_mut();
        let mut query = world.query::<(
            &sim_components::Position,
            &sim_components::TeamIdComponent,
            &RoleComponent,
        )>();
        let mut deepest_x: Option<f32> = None;
        for (pos, team, role) in query.iter(world) {
            if team.0 != HOME {
                continue;
            }
            if DEFENDER_ROLES.contains(&role.0) {
                deepest_x = Some(deepest_x.map_or(pos.0.x, |d| d.min(pos.0.x)));
            }
        }
        if let Some(x) = deepest_x {
            line_sum += x;
        }
    }

    // Resolved `pass` events carry kick and resolution positions.
    let log = sim.world().resource::<EventLog>();
    for event in &log.events {
        if let MatchEvent::Pass { payload, .. } = event {
            let start = Vec2::new(payload.start[0], payload.start[1]);
            let end = Vec2::new(payload.end[0], payload.end[1]);
            pass_lengths.push(start.distance(end));
        }
    }

    MatchMetrics {
        pass_lengths,
        line_mean_x: line_sum / ticks_as_f32(),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "24_000 ticks is exactly representable in f32"
)]
fn ticks_as_f32() -> f32 {
    OBSERVE_TICKS as f32
}

/// `Possession` shortens pass length vs `CounterAttack`.
#[test]
#[ignore = "multi-seed partial matches; run manually in release"]
fn tactic_possession_shortens_pass_length() {
    let mut possession_lengths: Vec<f32> = Vec::new();
    let mut counter_attack_lengths: Vec<f32> = Vec::new();
    for &seed in &SEEDS {
        let possession = observe(seed, baseline(), baseline());
        let counter_attack = observe(
            seed,
            setup(Mentality::Balance, Tactic::CounterAttack),
            baseline(),
        );
        possession_lengths.extend(possession.pass_lengths);
        counter_attack_lengths.extend(counter_attack.pass_lengths);
    }
    let poss_mean = mean(&possession_lengths);
    let ca_mean = mean(&counter_attack_lengths);
    println!(
        "possession pass length {poss_mean:.2}m vs counter_attack {ca_mean:.2}m over {} seeds",
        SEEDS.len()
    );
    assert!(
        !possession_lengths.is_empty() && !counter_attack_lengths.is_empty(),
        "both presets must produce resolved passes"
    );
    assert!(
        poss_mean < ca_mean,
        "Possession must shorten pass length vs CounterAttack: {poss_mean:.2} vs {ca_mean:.2}"
    );
}

/// `CounterAttack` lengthens pass length vs `Possession`.
#[test]
#[ignore = "multi-seed partial matches; run manually in release"]
fn tactic_counterattack_lengthens_pass_length() {
    let mut possession_lengths: Vec<f32> = Vec::new();
    let mut counter_attack_lengths: Vec<f32> = Vec::new();
    for &seed in &SEEDS {
        let possession = observe(seed, baseline(), baseline());
        let counter_attack = observe(
            seed,
            setup(Mentality::Balance, Tactic::CounterAttack),
            baseline(),
        );
        possession_lengths.extend(possession.pass_lengths);
        counter_attack_lengths.extend(counter_attack.pass_lengths);
    }
    let poss_mean = mean(&possession_lengths);
    let ca_mean = mean(&counter_attack_lengths);
    println!(
        "counter_attack pass length {ca_mean:.2}m vs possession {poss_mean:.2}m over {} seeds",
        SEEDS.len()
    );
    assert!(
        !possession_lengths.is_empty() && !counter_attack_lengths.is_empty(),
        "both presets must produce resolved passes"
    );
    assert!(
        ca_mean > poss_mean,
        "CounterAttack must lengthen pass length vs Possession: {ca_mean:.2} vs {poss_mean:.2}"
    );
}

/// `ParkTheBus` lowers the home defensive line vs `Possession` (lower
/// deepest-defender x since home attacks +x).
#[test]
#[ignore = "multi-seed partial matches; run manually in release"]
fn tactic_park_the_bus_lowers_line() {
    let mut baseline_x: Vec<f32> = Vec::new();
    let mut ptb_x: Vec<f32> = Vec::new();
    for &seed in &SEEDS {
        let b = observe(seed, baseline(), baseline());
        let ptb = observe(seed, setup(Mentality::Balance, Tactic::ParkTheBus), baseline());
        baseline_x.push(b.line_mean_x);
        ptb_x.push(ptb.line_mean_x);
    }
    let b_mean = mean(&baseline_x);
    let ptb_mean = mean(&ptb_x);
    println!(
        "park_the_bus line {ptb_mean:.2}m vs baseline {b_mean:.2}m over {} seeds",
        SEEDS.len()
    );
    assert!(
        ptb_mean < b_mean,
        "ParkTheBus must lower the defensive line vs Possession: {ptb_mean:.2} vs {b_mean:.2}"
    );
}

/// `Attack` raises the home defensive line vs `Balance`.
#[test]
#[ignore = "multi-seed partial matches; run manually in release"]
fn mentality_attack_raises_line() {
    let mut balance_x: Vec<f32> = Vec::new();
    let mut attack_x: Vec<f32> = Vec::new();
    for &seed in &SEEDS {
        let b = observe(seed, baseline(), baseline());
        let at = observe(seed, setup(Mentality::Attack, Tactic::Possession), baseline());
        balance_x.push(b.line_mean_x);
        attack_x.push(at.line_mean_x);
    }
    let b_mean = mean(&balance_x);
    let at_mean = mean(&attack_x);
    println!(
        "attack line {at_mean:.2}m vs balance {b_mean:.2}m over {} seeds",
        SEEDS.len()
    );
    assert!(
        at_mean > b_mean,
        "Attack must raise the line height vs Balance: {at_mean:.2} vs {b_mean:.2}"
    );
}

/// `Defend` lowers the home defensive line vs `Balance`.
#[test]
#[ignore = "multi-seed partial matches; run manually in release"]
fn mentality_defend_lowers_line() {
    let mut balance_x: Vec<f32> = Vec::new();
    let mut defend_x: Vec<f32> = Vec::new();
    for &seed in &SEEDS {
        let b = observe(seed, baseline(), baseline());
        let df = observe(seed, setup(Mentality::Defend, Tactic::Possession), baseline());
        balance_x.push(b.line_mean_x);
        defend_x.push(df.line_mean_x);
    }
    let b_mean = mean(&balance_x);
    let df_mean = mean(&defend_x);
    println!(
        "defend line {df_mean:.2}m vs balance {b_mean:.2}m over {} seeds",
        SEEDS.len()
    );
    assert!(
        df_mean < b_mean,
        "Defend must lower the line height vs Balance: {df_mean:.2} vs {b_mean:.2}"
    );
}

/// `HighPress` increases the press trigger distance: the `DistanceToPress`
/// consideration's logistic midpoint equals
/// `TacticParameters::press_trigger_distance()` (14 m for `HighPress`
/// vs 8 m for `Possession`). Pressing does not occur in match behaviour
/// with the current brain, so the parameter wiring is verified at the
/// consideration level (PRD §7.2: `HighPress` widens the press trigger).
#[test]
fn tactic_highpress_widens_press_trigger() {
    let balance = crate::simulation::brain_default::default_utility_brain_with(
        &sim_components::TacticParameters::from_setup(Mentality::Balance, Tactic::Possession),
    );
    let high_press = crate::simulation::brain_default::default_utility_brain_with(
        &sim_components::TacticParameters::from_setup(Mentality::Balance, Tactic::HighPress),
    );

    let balance_mid = press_trigger_midpoint(&balance);
    let hp_mid = press_trigger_midpoint(&high_press);
    println!(
        "high_press press trigger midpoint {hp_mid:.1}m vs balance {balance_mid:.1}m"
    );
    assert!((balance_mid - 8.0).abs() < f32::EPSILON, "Balance press trigger midpoint must be 8 m");
    assert!((hp_mid - 14.0).abs() < f32::EPSILON, "HighPress press trigger midpoint must be 14 m");
    assert!(
        hp_mid > balance_mid,
        "HighPress must widen the press trigger vs Possession: {hp_mid} vs {balance_mid}"
    );
}

/// Return the `DistanceToPress` consideration's logistic midpoint for the
/// `Press` action of `brain`, if present.
fn press_trigger_midpoint(brain: &sim_ai_player::UtilityBrain) -> f32 {
    for action in &brain.actions {
        if matches!(
            action.intent,
            sim_components::Intent::Action(sim_components::ActionIntent::Press(_))
        ) {
            for consideration in &action.considerations {
                if matches!(
                    consideration,
                    sim_ai_player::Consideration::DistanceToPress { .. }
                )
                    && let sim_ai_core::ResponseCurve::Logistic { midpoint, .. } =
                        consideration.curve()
                {
                    return *midpoint;
                }
            }
        }
    }
    panic!("Press action has no DistanceToPress consideration");
}
