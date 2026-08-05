use covopt_macro::covopt_param;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum AstNode {
    N,
    Constant(f64),
    Pi,
    E,
    Phi,
    Sqrt2,
    Add(Box<AstNode>, Box<AstNode>),
    Sub(Box<AstNode>, Box<AstNode>),
    Mul(Box<AstNode>, Box<AstNode>),
    Div(Box<AstNode>, Box<AstNode>),
    Pow(Box<AstNode>, f64),
    Sin(Box<AstNode>),
    Cos(Box<AstNode>),
    Exp(Box<AstNode>),
    Log(Box<AstNode>),
    Sqrt(Box<AstNode>),
}

impl AstNode {
    pub fn evaluate(&self, n: f64) -> f64 {
        match self {
            AstNode::N => n,
            AstNode::Constant(c) => *c,
            AstNode::Pi => std::f64::consts::PI,
            AstNode::E => std::f64::consts::E,
            AstNode::Phi => 1.618033988749895,
            AstNode::Sqrt2 => std::f64::consts::SQRT_2,
            AstNode::Add(l, r) => l.evaluate(n) + r.evaluate(n),
            AstNode::Sub(l, r) => l.evaluate(n) - r.evaluate(n),
            AstNode::Mul(l, r) => l.evaluate(n) * r.evaluate(n),
            AstNode::Div(l, r) => {
                let den = r.evaluate(n);
                if den.abs() < covopt_param!("M_24_31", 1e-9) {
                    covopt_param!("M_25_20", 1e9)
                } else {
                    l.evaluate(n) / den
                }
            }
            AstNode::Pow(l, p) => l.evaluate(n).powf(*p),
            AstNode::Sin(inner) => inner.evaluate(n).sin(),
            AstNode::Cos(inner) => inner.evaluate(n).cos(),
            AstNode::Exp(inner) => inner.evaluate(n).exp(),
            AstNode::Log(inner) => {
                let v = inner.evaluate(n);
                if v <= 0.0 { -1e9 } else { v.ln() }
            },
            AstNode::Sqrt(inner) => {
                let v = inner.evaluate(n);
                if v < 0.0 { 0.0 } else { v.sqrt() }
            },
        }
    }

    pub fn mutate(&mut self, seed: &mut usize) {
        *seed = seed
            .wrapping_mul(covopt_param!("M_35_34", 1664525))
            .wrapping_add(covopt_param!("M_35_56", 1013904223));
        let rand = *seed % covopt_param!("M_36_27", 100);

        match self {
            AstNode::Constant(c) => {
                if rand < covopt_param!("M_40_26", 50) {
                    *c += covopt_param!("M_41_26", 0.5);
                } else {
                    *c -= covopt_param!("M_43_26", 0.5);
                }
            }
            AstNode::Pow(_, p) => {
                if rand < covopt_param!("M_47_26", 50) {
                    *p += 1.0;
                } else {
                    *p -= 1.0;
                }
            }
            AstNode::Add(l, r) | AstNode::Sub(l, r) | AstNode::Mul(l, r) | AstNode::Div(l, r) => {
                if rand < covopt_param!("M_54_26", 50) {
                    l.mutate(seed);
                } else {
                    r.mutate(seed);
                }
            }
            AstNode::Sin(inner) | AstNode::Cos(inner) | AstNode::Exp(inner) | AstNode::Log(inner) | AstNode::Sqrt(inner) => {
                inner.mutate(seed);
            }
            AstNode::Pi | AstNode::E | AstNode::Phi | AstNode::Sqrt2 => {
                // Constants cannot be mutated, randomly switch to another constant
                if rand < 25 {
                    *self = AstNode::Constant(3.1415);
                }
            }
            AstNode::N => {
                if rand < covopt_param!("M_61_26", 10) {
                    *self = AstNode::Constant(1.0);
                }
            }
        }
    }
}

impl fmt::Display for AstNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AstNode::N => write!(f, "N"),
            AstNode::Constant(c) => write!(f, "{:.2}", c),
            AstNode::Pi => write!(f, "π"),
            AstNode::E => write!(f, "e"),
            AstNode::Phi => write!(f, "φ"),
            AstNode::Sqrt2 => write!(f, "√2"),
            AstNode::Add(l, r) => write!(f, "({} + {})", l, r),
            AstNode::Sub(l, r) => write!(f, "({} - {})", l, r),
            AstNode::Mul(l, r) => write!(f, "{} * {}", l, r),
            AstNode::Div(l, r) => write!(f, "({} / {})", l, r),
            AstNode::Pow(l, p) => write!(f, "{}^{:.1}", l, p),
            AstNode::Sin(inner) => write!(f, "sin({})", inner),
            AstNode::Cos(inner) => write!(f, "cos({})", inner),
            AstNode::Exp(inner) => write!(f, "exp({})", inner),
            AstNode::Log(inner) => write!(f, "log({})", inner),
            AstNode::Sqrt(inner) => write!(f, "sqrt({})", inner),
        }
    }
}

pub struct SymbolicRegressor;

impl SymbolicRegressor {
    pub fn formalize(data: &[(usize, u64)]) -> String {
        if data.is_empty() {
            return "0".to_string();
        }

        let mut pool = vec![
            // Basic Add
            AstNode::Add(Box::new(AstNode::N), Box::new(AstNode::Constant(1.0))),
            AstNode::Mul(Box::new(AstNode::Constant(1.0)), Box::new(AstNode::N)),
            AstNode::Pow(Box::new(AstNode::N), 2.0),
            // Polynomial / Padé Approximant Template (C1*x + C2*x^3)
            AstNode::Add(
                Box::new(AstNode::Mul(
                    Box::new(AstNode::Constant(1.0)),
                    Box::new(AstNode::N),
                )),
                Box::new(AstNode::Mul(
                    Box::new(AstNode::Constant(0.16666)), // 1/6
                    Box::new(AstNode::Pow(Box::new(AstNode::N), 3.0)),
                )),
            ),
            // Transcendentals
            AstNode::Sin(Box::new(AstNode::Mul(Box::new(AstNode::Pi), Box::new(AstNode::N)))),
        ];

        let mut best_ast = pool[0].clone();
        let mut min_error = f64::MAX;
        let mut seed: usize = covopt_param!("M_109_30", 12345);

        for _generation in 0..covopt_param!("M_111_30", 5000) {
            for ast in &mut pool {
                // Mutate some trees
                seed = seed
                    .wrapping_mul(covopt_param!("M_114_41", 1664525))
                    .wrapping_add(covopt_param!("M_114_63", 1013904223));
                if seed % covopt_param!("M_115_26", 10) < covopt_param!("M_115_31", 3) {
                    let mut new_ast = ast.clone();
                    new_ast.mutate(&mut seed);
                    *ast = new_ast;
                }

                // Calculate fitness (Mean Squared Error)
                let mut error = 0.0;
                for &(n, hit_count) in data {
                    let pred = ast.evaluate(n as f64);
                    let diff = pred - (hit_count as f64);
                    error += diff * diff;
                }

                if error < min_error {
                    min_error = error;
                    best_ast = ast.clone();
                }
            }

            // Reproduce the best
            if min_error < 1.0 {
                break; // Perfect fit found
            }

            pool[0] = best_ast.clone(); // Elitism
        }

        format!("f(N) = {} (MSE: {:.4})", best_ast, min_error)
    }
}
