

#[derive(Debug, Clone)]
pub struct Consideration {
    pub name: String,
    pub curve: ResponseCurve,
}

#[derive(Debug, Clone)]
pub enum ResponseCurve {
    Linear { min: f32, max: f32 },
    Logistic { midpoint: f32, steepness: f32 },
    Step { threshold: f32, below: f32, above: f32 },
}

impl ResponseCurve {
    pub fn evaluate(&self, input: f32) -> f32 {
        match self {
            ResponseCurve::Linear { min, max } => {
                (input - min) / (max - min).clamp(0.0, 1.0)
            }
            ResponseCurve::Logistic { midpoint, steepness } => {
                let x = steepness * (input - midpoint);
                1.0 / (1.0 + (-x).exp())
            }
            ResponseCurve::Step {
                threshold,
                below,
                above,
            } => {
                if input < *threshold {
                    *below
                } else {
                    *above
                }
            }
        }
    }
}

pub fn geometric_mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let product: f32 = values.iter().map(|v| v.max(1e-4)).product();
    product.powf(1.0 / values.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_curves() {
        let linear = ResponseCurve::Linear {
            min: 0.0,
            max: 1.0,
        };
        assert_eq!(linear.evaluate(0.5), 0.5);

        let logistic = ResponseCurve::Logistic {
            midpoint: 0.5,
            steepness: 10.0,
        };
        let result = logistic.evaluate(0.5);
        assert!((result - 0.5).abs() < 0.01);

        let step = ResponseCurve::Step {
            threshold: 0.5,
            below: 0.0,
            above: 1.0,
        };
        assert_eq!(step.evaluate(0.3), 0.0);
        assert_eq!(step.evaluate(0.7), 1.0);
    }

    #[test]
    fn test_geometric_mean() {
        let values = vec![1.0, 2.0, 3.0];
        let result = geometric_mean(&values);
        assert!((result - 1.817).abs() < 0.01);
    }
}