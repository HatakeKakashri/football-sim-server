/// A utility-AI score in the closed unit interval `[0.0, 1.0]`.
///
/// Constructed via `Score::new(input)` which clamps the input into range.
/// Use `.raw()` only when arithmetic genuinely requires raw `f32` (e.g.
/// log-space geometric mean combiners); otherwise propagate the `Score` so
/// the type system enforces the invariant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score(f32);

#[allow(
    clippy::use_self,
    reason = "named constants like ZERO/ONE read more clearly than Self(0.0)"
)]
#[allow(
    clippy::missing_const_for_fn,
    reason = "f32::is_nan / clamp const-stability is recent; non-const keeps MSRV floor lower"
)]
impl Score {
    pub const ZERO: Score = Score(0.0);
    pub const ONE: Score = Score(1.0);

    /// Construct a Score, clamping the input into `[0.0, 1.0]`.
    #[must_use]
    pub fn new(value: f32) -> Self {
        if value.is_nan() {
            return Score(0.0);
        }
        Score(value.clamp(0.0, 1.0))
    }

    /// Lossy accessor returning the underlying `f32`. Use only when
    /// downstream arithmetic requires raw float values.
    #[must_use]
    pub fn raw(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Consideration {
    pub name: String,
    pub curve: ResponseCurve,
}

#[derive(Debug, Clone)]
pub enum ResponseCurve {
    Linear {
        min: f32,
        max: f32,
    },
    Logistic {
        midpoint: f32,
        steepness: f32,
    },
    Step {
        threshold: f32,
        below: f32,
        above: f32,
    },
}

impl ResponseCurve {
    /// Evaluate the curve at the given input, returning a `Score` clamped
    /// to `[0.0, 1.0]`.
    ///
    /// `Linear` divides by `(max - min).max(f32::EPSILON)` so the
    /// `max <= min` degenerate case yields `Score::ZERO` instead of NaN.
    /// The previous implementation incorrectly clamped the *denominator*
    /// due to operator precedence — fixed in Phase E PR 2.
    #[must_use]
    pub fn evaluate(&self, input: f32) -> Score {
        match self {
            Self::Linear { min, max } => {
                let denom = (max - min).max(f32::EPSILON);
                let raw = (input - min) / denom;
                Score::new(raw)
            }
            Self::Logistic {
                midpoint,
                steepness,
            } => {
                let x = steepness * (input - midpoint);
                Score::new(1.0 / (1.0 + (-x).exp()))
            }
            Self::Step {
                threshold,
                below,
                above,
            } => Score::new(if input < *threshold { *below } else { *above }),
        }
    }
}

/// Geometric mean via log-space sum, so long inputs can't underflow the product.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "slice length is far below 2^24, so the usize -> f32 cast is exact"
)]
pub fn geometric_mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let log_sum: f32 = values.iter().map(|v| v.max(1e-4).ln()).sum();
    (log_sum / values.len() as f32).exp()
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests assert exact Score values after clamping"
)]
mod tests {
    use super::*;

    #[test]
    fn test_response_curves() {
        let linear = ResponseCurve::Linear { min: 0.0, max: 1.0 };
        assert_eq!(linear.evaluate(0.5).raw(), 0.5);

        let logistic = ResponseCurve::Logistic {
            midpoint: 0.5,
            steepness: 10.0,
        };
        let result = logistic.evaluate(0.5);
        assert!((result.raw() - 0.5).abs() < 0.01);

        let step = ResponseCurve::Step {
            threshold: 0.5,
            below: 0.0,
            above: 1.0,
        };
        assert_eq!(step.evaluate(0.3), Score::new(0.0));
        assert_eq!(step.evaluate(0.7), Score::new(1.0));
    }

    #[test]
    fn test_geometric_mean() {
        let values = vec![1.0, 2.0, 3.0];
        let result = geometric_mean(&values);
        assert!((result - 1.817).abs() < 0.01);
    }

    #[test]
    fn test_score_new_clamps_to_unit_interval() {
        let below = Score::new(-0.5);
        let above = Score::new(1.5);
        let inside = Score::new(0.5);
        assert_eq!(below, Score::ZERO);
        assert_eq!(above, Score::ONE);
        assert_eq!(inside.raw(), 0.5);
    }

    #[test]
    fn test_score_constants() {
        assert_eq!(Score::ZERO.raw(), 0.0);
        assert_eq!(Score::ONE.raw(), 1.0);
    }

    #[test]
    fn test_score_partial_eq() {
        assert_eq!(Score::new(0.5), Score::new(0.5));
        assert_ne!(Score::new(0.5), Score::new(0.4));
    }

    #[test]
    fn test_linear_response_handles_max_eq_min() {
        // Phase E PR 2 regression: before the fix, the Linear arm clamped
        // `(max - min)` (the wrong operand) so `max == min` produced NaN/inf.
        // After the fix, the denominator is `(max - min).max(f32::EPSILON)`
        // and the result is clamped via `Score::new`, so a degenerate
        // `max == min` linear curve returns `Score::ZERO` instead of NaN.
        let degenerate = ResponseCurve::Linear { min: 0.5, max: 0.5 };
        let score = degenerate.evaluate(0.5);
        assert!(!score.raw().is_nan(), "Linear with max==min returned NaN");
        assert_eq!(score, Score::ZERO);
    }

    #[test]
    fn test_linear_response_clamped_at_unit_interval() {
        // Linear curve that overshoots its bounds should still return a
        // valid Score in [0,1] rather than raw f32.
        let curve = ResponseCurve::Linear { min: 0.0, max: 1.0 };
        assert_eq!(curve.evaluate(2.0), Score::ONE);
        assert_eq!(curve.evaluate(-2.0), Score::ZERO);
    }
}
