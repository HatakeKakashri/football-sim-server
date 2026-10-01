//! `Consideration::name` must be a stable `snake_case` identifier per variant.

use sim_ai_core::ResponseCurve;

use crate::Consideration;

/// One `(variant, expected name)` row per `Consideration` variant.
macro_rules! name_cases {
    ( $curve:expr; $( $variant:ident => $name:literal ),+ $(,)? ) => {
        vec![ $( (Consideration::$variant { weight: 1.0, curve: $curve.clone() }, $name) ),+ ]
    };
}

#[test]
fn every_variant_has_its_snake_case_name() {
    let curve = ResponseCurve::Linear { min: 0.0, max: 1.0 };
    let cases: Vec<(Consideration, &str)> = name_cases! { curve;
        DistanceToTarget => "distance_to_target",
        DistanceToBall => "distance_to_ball",
        Stamina => "stamina",
        PitchControlAtBall => "pitch_control_at_ball",
        PassAngleClear => "pass_angle_clear",
        TeammateDistance => "teammate_distance",
        TeammateSpace => "teammate_space",
        DistanceToGoal => "distance_to_goal",
        GoalAngle => "goal_angle",
        DefenderPressure => "defender_pressure",
        DistanceToOpponent => "distance_to_opponent",
        SkillDiff => "skill_diff",
        DistanceToMarked => "distance_to_marked",
        DefensivePosition => "defensive_position",
        DistanceToPress => "distance_to_press",
        SpaceAhead => "space_ahead",
        TeammateBall => "teammate_ball",
        FormationDiscipline => "formation_discipline",
    };
    assert_eq!(cases.len(), 18, "one case per Consideration variant");
    for (consideration, expected) in cases {
        assert_eq!(consideration.name(), expected);
    }
}
