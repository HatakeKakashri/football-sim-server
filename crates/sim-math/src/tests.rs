#![allow(clippy::float_cmp, reason = "tests assert exact arithmetic results from integer-literal vector operands")]
use super::*;
use crate::slots::slot_to_row_col;

#[test]
fn test_vec2_operations() {
    let v1 = Vec2::new(1.0, 2.0);
    let v2 = Vec2::new(3.0, 4.0);

    assert_eq!(v1 + v2, Vec2::new(4.0, 6.0));
    assert_eq!(v2 - v1, Vec2::new(2.0, 2.0));
    assert_eq!(v1 * 2.0, Vec2::new(2.0, 4.0));
    assert_eq!(2.0 * v1, Vec2::new(2.0, 4.0));
    assert_eq!(v1.dot(v2), 11.0);
    assert_eq!(v1.length_squared(), 5.0);
}

#[test]
fn test_swept_proximity_static_no_acquire() {
    // Player at (0,0), ball at (5,0), both stationary.
    // Distance is 5m > 1.5m acquisition radius.
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::zero(),
        Vec2::new(5.0, 0.0),
        Vec2::zero(),
        1.5,
    );
    assert_eq!(result.min_distance, 5.0);
    assert!(!result.acquired);
}

#[test]
fn test_swept_proximity_static_acquire() {
    // Player at (0,0), ball at (1.0, 0), both stationary.
    // Distance is 1.0m < 1.5m acquisition radius.
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::zero(),
        Vec2::new(1.0, 0.0),
        Vec2::zero(),
        1.5,
    );
    assert_eq!(result.min_distance, 1.0);
    assert!(result.acquired);
}

#[test]
fn test_swept_proximity_player_crosses_ball() {
    // Player moves from (0,0) to (5,0) — crosses the ball at (2.5, 0).
    // Ball stays at (2.5, 0). Minimum distance is 0 at t=0.5.
    let result = swept_proximity(
        Vec2::new(0.0, 0.0), // player start
        Vec2::new(5.0, 0.0), // player velocity: 5m/tick
        Vec2::new(2.5, 0.0), // ball start
        Vec2::zero(),        // ball stationary
        1.5,
    );
    assert!((result.min_distance - 0.0).abs() < 1e-6);
    assert!(result.acquired);
}

#[test]
fn test_swept_proximity_player_misses_ball() {
    // Player moves from (0,0) to (5,0), ball stays at (0, 5).
    // Minimum distance is 5m (player passes 5m away).
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::new(5.0, 0.0),
        Vec2::new(0.0, 5.0),
        Vec2::zero(),
        1.5,
    );
    assert_eq!(result.min_distance, 5.0);
    assert!(!result.acquired);
}

#[test]
fn test_swept_proximity_both_moving_towards() {
    // Player at (0,0) moving right at 3m/tick, ball at (5,0) moving left at 2m/tick.
    // They approach each other. Meeting point: t=1, position (1,0).
    // Minimum distance is 0 (they meet).
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::new(3.0, 0.0),
        Vec2::new(5.0, 0.0),
        Vec2::new(-2.0, 0.0),
        1.5,
    );
    assert!((result.min_distance - 0.0).abs() < 1e-6);
    assert!(result.acquired);
}

#[test]
fn test_swept_proximity_ball_moving_past_stationary_player() {
    // Ball moving from (0,0) to (5,0), player stationary at (2.5, 0).
    // Ball passes directly through player at t=0.5.
    let result = swept_proximity(
        Vec2::new(2.5, 0.0),
        Vec2::zero(),
        Vec2::new(0.0, 0.0),
        Vec2::new(5.0, 0.0),
        1.5,
    );
    assert!((result.min_distance - 0.0).abs() < 1e-6);
    assert!(result.acquired);
}

#[test]
fn test_swept_proximity_endpoint_acquire() {
    // Player starts at (1.4, 0) moving to (1.6, 0), ball at (0, 0) stationary.
    // Player's path comes within 1.4m at t=0, which is < 1.5m.
    let result = swept_proximity(
        Vec2::new(1.4, 0.0),
        Vec2::new(0.2, 0.0),
        Vec2::new(0.0, 0.0),
        Vec2::zero(),
        1.5,
    );
    assert!((result.min_distance - 1.4).abs() < 1e-5);
    assert!(result.acquired);
}

#[test]
fn test_swept_proximity_past_endpoint_acquire() {
    // Player starts at (0, 0) moving to (3.0, 0), ball at (0, 1.4) stationary.
    // Minimum distance is 1.4 at t=0 (player passes 1.4m from ball).
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::new(3.0, 0.0),
        Vec2::new(0.0, 1.4),
        Vec2::zero(),
        1.5,
    );
    assert!((result.min_distance - 1.4).abs() < 1e-6);
    assert!(result.acquired);
}

#[test]
fn test_swept_proximity_past_endpoint_no_acquire() {
    // Player starts at (0, 0) moving to (3.0, 0), ball at (0, 2.0) stationary.
    // Minimum distance is 2.0 at t=0, which is > 1.5m.
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::new(3.0, 0.0),
        Vec2::new(0.0, 2.0),
        Vec2::zero(),
        1.5,
    );
    assert!((result.min_distance - 2.0).abs() < 1e-6);
    assert!(!result.acquired);
}

#[test]
fn test_swept_proximity_diagonal_motion() {
    // Player moves diagonally from (0,0) to (3,4) (length 5m at angle).
    // Ball at (2.5, 2.0). Closest approach at t=0.62:
    // player at (1.86, 2.48), distance = sqrt(0.64 + 0.2304) = sqrt(0.8704) ≈ 0.933m
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::new(3.0, 4.0),
        Vec2::new(2.5, 2.0),
        Vec2::zero(),
        1.5,
    );
    assert!((result.min_distance - 0.8).abs() < 1e-4);
    assert!(result.acquired);
}

#[test]
fn test_swept_proximity_inner_endpoint() {
    // Player moves from (0,0) to (5,0), ball moves from (3,0) to (4,0).
    // Relative motion: player starts 3m behind and closes 1m gap.
    // They overlap at t=0.6: player at 3.0, ball at 3.6.
    // Min distance should be 0 (they cross paths).
    let result = swept_proximity(
        Vec2::new(0.0, 0.0),
        Vec2::new(5.0, 0.0),
        Vec2::new(3.0, 0.0),
        Vec2::new(1.0, 0.0),
        1.5,
    );
    assert!(result.min_distance < 1e-5);
    assert!(result.acquired);
}

// === PositionSlot and fit distance tests ===

#[test]
fn test_position_slot_grid_coordinates() {
    // Verify the (row, col) mapping for the 5×3 grid.
    assert_eq!(slot_to_row_col(PositionSlot::GK), (0, 0)); // Sentinel

    // Row 1 (defensive line)
    assert_eq!(slot_to_row_col(PositionSlot::DL), (1, 1));
    assert_eq!(slot_to_row_col(PositionSlot::DC), (1, 2));
    assert_eq!(slot_to_row_col(PositionSlot::DR), (1, 3));

    // Row 2 (defensive midfield)
    assert_eq!(slot_to_row_col(PositionSlot::DML), (2, 1));
    assert_eq!(slot_to_row_col(PositionSlot::DMC), (2, 2));
    assert_eq!(slot_to_row_col(PositionSlot::DMR), (2, 3));

    // Row 3 (midfield)
    assert_eq!(slot_to_row_col(PositionSlot::ML), (3, 1));
    assert_eq!(slot_to_row_col(PositionSlot::MC), (3, 2));
    assert_eq!(slot_to_row_col(PositionSlot::MR), (3, 3));

    // Row 4 (attacking midfield)
    assert_eq!(slot_to_row_col(PositionSlot::AML), (4, 1));
    assert_eq!(slot_to_row_col(PositionSlot::AMC), (4, 2));
    assert_eq!(slot_to_row_col(PositionSlot::AMR), (4, 3));

    // Row 5 (striker)
    assert_eq!(slot_to_row_col(PositionSlot::ST), (5, 2));
}

#[test]
fn test_chebyshev_distance_same_slot() {
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::MC), 0);
    assert_eq!(chebyshev_distance(PositionSlot::ST, PositionSlot::ST), 0);
}

#[test]
fn test_chebyshev_distance_distance_1() {
    // Adjacent cells horizontally
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::ML), 1);
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::MR), 1);

    // Adjacent cells vertically
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::DMC), 1);
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::AMC), 1);

    // Diagonal adjacency (MC at row 3 col 2)
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::ML), 1); // same row, col diff 1
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::MR), 1); // same row, col diff 1
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::AMC), 1); // row diff 1, same col
    assert_eq!(chebyshev_distance(PositionSlot::MC, PositionSlot::AMR), 1); // diagonally adjacent: MC=(3,2), AMR=(4,3), diff=(1,1), max=1
}

#[test]
fn test_chebyshev_distance_distance_2() {
    // Two steps horizontally
    assert_eq!(chebyshev_distance(PositionSlot::ML, PositionSlot::MR), 2);

    // Two steps vertically
    assert_eq!(chebyshev_distance(PositionSlot::DMC, PositionSlot::AMC), 2);

    // Diagonal (2,2) -> (3,3) is distance 1, but (1,1) -> (3,3) is distance 2
    assert_eq!(chebyshev_distance(PositionSlot::DL, PositionSlot::MC), 2);
}

#[test]
fn test_chebyshev_distance_gk_sentinel() {
    // GK maps to the off-grid sentinel (0,0), so it must be at non-zero
    // distance from every other slot (in both directions) and at distance
    // 0 from itself.
    assert_eq!(chebyshev_distance(PositionSlot::GK, PositionSlot::GK), 0);
    for slot in [
        PositionSlot::DL, PositionSlot::DC, PositionSlot::DR,
        PositionSlot::DML, PositionSlot::DMC, PositionSlot::DMR,
        PositionSlot::ML, PositionSlot::MC, PositionSlot::MR,
        PositionSlot::AML, PositionSlot::AMC, PositionSlot::AMR,
        PositionSlot::ST,
    ] {
        assert!(chebyshev_distance(PositionSlot::GK, slot) > 0, "GK -> {slot:?}");
        assert_eq!(
            chebyshev_distance(PositionSlot::GK, slot),
            chebyshev_distance(slot, PositionSlot::GK),
            "distance must be symmetric for {slot:?}"
        );
    }
}

#[test]
fn test_min_fit_distance_basic() {
    let tuning = FitMultipliers::default_assumptions();

    // Player trained at MC, playing at MC -> distance 0 -> multiplier 1.00
    let trained = vec![PositionSlot::MC];
    let dist = min_fit_distance(PositionSlot::MC, &trained);
    assert_eq!(dist, Some(0));
    assert_eq!(fit_multiplier(PositionSlot::MC, &trained, &tuning), 1.00);

    // Player trained at MC, playing at ML -> distance 1 -> multiplier 0.92
    let trained = vec![PositionSlot::MC];
    let dist = min_fit_distance(PositionSlot::ML, &trained);
    assert_eq!(dist, Some(1));
    assert_eq!(fit_multiplier(PositionSlot::ML, &trained, &tuning), 0.92);

    // Player trained at MC, playing at DL -> distance 2 -> multiplier 0.75
    let trained = vec![PositionSlot::MC];
    let dist = min_fit_distance(PositionSlot::DL, &trained);
    assert_eq!(dist, Some(2));
    assert_eq!(fit_multiplier(PositionSlot::DL, &trained, &tuning), 0.75);
}

#[test]
fn test_min_fit_distance_multiple_trained_positions() {
    let tuning = FitMultipliers::default_assumptions();

    // Player trained at both MC and MR, playing at ML (closest is MC, distance 1)
    let trained = vec![PositionSlot::MC, PositionSlot::MR];
    let dist = min_fit_distance(PositionSlot::ML, &trained);
    assert_eq!(dist, Some(1));
    assert_eq!(fit_multiplier(PositionSlot::ML, &trained, &tuning), 0.92);

    // Player trained at DC and ST, playing at MC (DC is distance 2, ST is distance 3, so min is 2)
    let trained = vec![PositionSlot::DC, PositionSlot::ST];
    let dist = min_fit_distance(PositionSlot::MC, &trained);
    assert_eq!(dist, Some(2));
    assert_eq!(fit_multiplier(PositionSlot::MC, &trained, &tuning), 0.75);
}

#[test]
fn test_min_fit_distance_gk() {
    // GK slot should return None (not on the 13-slot grid)
    let trained = vec![PositionSlot::GK];
    let dist = min_fit_distance(PositionSlot::GK, &trained);
    assert_eq!(dist, None);

    let trained = vec![PositionSlot::DL, PositionSlot::DC, PositionSlot::DR];
    let dist = min_fit_distance(PositionSlot::GK, &trained);
    assert_eq!(dist, None);
}

#[test]
fn test_min_fit_distance_empty() {
    // Empty trained positions returns None
    let trained: Vec<PositionSlot> = vec![];
    let dist = min_fit_distance(PositionSlot::MC, &trained);
    assert_eq!(dist, None);
}

#[test]
fn test_fit_multiplier_with_custom_tuning() {
    let custom = FitMultipliers {
        at_distance_0: 1.00,
        at_distance_1: 0.85,  // More lenient
        at_distance_2_or_more: 0.60, // More severe
    };

    let trained = vec![PositionSlot::MC];

    assert_eq!(fit_multiplier(PositionSlot::MC, &trained, &custom), 1.00);
    assert_eq!(fit_multiplier(PositionSlot::ML, &trained, &custom), 0.85);
    assert_eq!(fit_multiplier(PositionSlot::DL, &trained, &custom), 0.60);
}

#[test]
fn test_effective_attribute() {
    // Raw attribute 100 with multiplier 1.00 -> 100.0
    assert!((effective_attribute(100, 1.00) - 100.0).abs() < f32::EPSILON);
    // Raw attribute 100 with multiplier 0.92 -> 92.0
    assert!((effective_attribute(100, 0.92) - 92.0).abs() < f32::EPSILON);
    // Raw attribute 100 with multiplier 0.75 -> 75.0
    assert!((effective_attribute(100, 0.75) - 75.0).abs() < f32::EPSILON);
    // Raw attribute 50 with multiplier 0.75 -> 37.5
    assert!((effective_attribute(50, 0.75) - 37.5).abs() < f32::EPSILON);
}

#[test]
fn test_tuning_default() {
    let tuning = Tuning::default();
    assert_eq!(tuning.fit_multipliers.at_distance_0, 1.00);
    assert_eq!(tuning.fit_multipliers.at_distance_1, 0.92);
    assert_eq!(tuning.fit_multipliers.at_distance_2_or_more, 0.75);
    assert_eq!(tuning.curve_params.len(), CURVE_PARAM_COUNT);
    assert_eq!(tuning.tackle_recovery_ticks, 90);
    assert!((0.0..=1.0).contains(&tuning.stamina_drain_per_action));
    assert!((0.0..=1.0).contains(&tuning.stamina_recovery_rate));
    assert!(tuning.press_drain_multiplier > 0.0);
    assert!(tuning.tackle_drain_multiplier > 0.0);
}

#[test]
fn test_curve_midpoint_returns_mid_range() {
    let params = CurveParams {
        midpoint: 100.0,
        steepness: 6.0,
        min: 0.0,
        max: 1.0,
        rising: true,
    };
    assert!((curve(100, &params) - 0.5).abs() < 1e-6);
}

#[test]
fn test_curve_clamps_attribute_to_unit_interval() {
    let params = CurveParams {
        midpoint: 100.0,
        steepness: 6.0,
        min: 0.0,
        max: 1.0,
        rising: true,
    };
    assert!((curve(-50, &params) - curve(0, &params)).abs() < 1e-6);
    assert!((curve(250, &params) - curve(200, &params)).abs() < 1e-6);
}

#[test]
fn test_curve_is_strictly_monotonic_rising() {
    let params = CurveParams {
        midpoint: 100.0,
        steepness: 6.0,
        min: 0.05,
        max: 0.95,
        rising: true,
    };
    let mut prev = curve(0, &params);
    for attr in 1..=200 {
        let value = curve(attr, &params);
        assert!(value > prev, "curve not strictly rising at attribute {attr}");
        prev = value;
    }
}

#[test]
fn test_curve_is_strictly_monotonic_falling() {
    let params = CurveParams {
        midpoint: 100.0,
        steepness: 6.0,
        min: 0.02,
        max: 0.70,
        rising: false,
    };
    let mut prev = curve(0, &params);
    for attr in 1..=200 {
        let value = curve(attr, &params);
        assert!(value < prev, "curve not strictly falling at attribute {attr}");
        prev = value;
    }
}

#[test]
fn test_curve_output_stays_within_min_max() {
    let params = CurveParams {
        midpoint: 100.0,
        steepness: 6.0,
        min: 0.02,
        max: 0.70,
        rising: false,
    };
    for attr in 0..=200 {
        let value = curve(attr, &params);
        assert!((0.02..=0.70).contains(&value), "attribute {attr}: {value}");
    }
}

#[test]
fn test_curve_falling_is_reverse_of_rising() {
    // A falling curve with the same span is the mirror of the rising one.
    let rising = CurveParams {
        midpoint: 100.0,
        steepness: 6.0,
        min: 0.05,
        max: 0.95,
        rising: true,
    };
    let falling = CurveParams {
        midpoint: 100.0,
        steepness: 6.0,
        min: 0.05,
        max: 0.95,
        rising: false,
    };
    for attr in 0..=200 {
        assert!(
            (curve(attr, &rising) + curve(attr, &falling) - 1.0).abs() < 1e-6,
            "attribute {attr}"
        );
    }
}
