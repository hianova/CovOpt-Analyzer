use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum AstNode {
    N,
    Constant(f64),
    Add(Box<AstNode>, Box<AstNode>),
    Sub(Box<AstNode>, Box<AstNode>),
    Mul(Box<AstNode>, Box<AstNode>),
    Div(Box<AstNode>, Box<AstNode>),
    Pow(Box<AstNode>, f64),
}

impl AstNode {
    pub fn evaluate(&self, n: f64) -> f64 {
        match self {
            AstNode::N => n,
            AstNode::Constant(c) => *c,
            AstNode::Add(l, r) => l.evaluate(n) + r.evaluate(n),
            AstNode::Sub(l, r) => l.evaluate(n) - r.evaluate(n),
            AstNode::Mul(l, r) => l.evaluate(n) * r.evaluate(n),
            AstNode::Div(l, r) => {
                let den = r.evaluate(n);
                if den.abs() < 1e-9 {
                    1e9
                } else {
                    l.evaluate(n) / den
                }
            }
            AstNode::Pow(l, p) => l.evaluate(n).powf(*p),
        }
    }

    pub fn mutate(&mut self, seed: &mut usize) {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let rand = *seed % 100;

        match self {
            AstNode::Constant(c) => {
                if rand < 50 {
                    *c += 0.5;
                } else {
                    *c -= 0.5;
                }
            }
            AstNode::Pow(_, p) => {
                if rand < 50 {
                    *p += 1.0;
                } else {
                    *p -= 1.0;
                }
            }
            AstNode::Add(l, r) | AstNode::Sub(l, r) | AstNode::Mul(l, r) | AstNode::Div(l, r) => {
                if rand < 50 {
                    l.mutate(seed);
                } else {
                    r.mutate(seed);
                }
            }
            AstNode::N => {
                if rand < 10 {
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
            AstNode::Add(l, r) => write!(f, "({} + {})", l, r),
            AstNode::Sub(l, r) => write!(f, "({} - {})", l, r),
            AstNode::Mul(l, r) => write!(f, "{} * {}", l, r),
            AstNode::Div(l, r) => write!(f, "({} / {})", l, r),
            AstNode::Pow(l, p) => write!(f, "{}^{:.1}", l, p),
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
            AstNode::Add(Box::new(AstNode::N), Box::new(AstNode::Constant(1.0))),
            AstNode::Mul(Box::new(AstNode::Constant(1.0)), Box::new(AstNode::N)),
            AstNode::Pow(Box::new(AstNode::N), 2.0),
            AstNode::Add(
                Box::new(AstNode::Mul(
                    Box::new(AstNode::Constant(1.0)),
                    Box::new(AstNode::Pow(Box::new(AstNode::N), 2.0)),
                )),
                Box::new(AstNode::Mul(
                    Box::new(AstNode::Constant(1.0)),
                    Box::new(AstNode::N),
                )),
            ),
        ];

        let mut best_ast = pool[0].clone();
        let mut min_error = f64::MAX;
        let mut seed: usize = 12345;

        for _generation in 0..5000 {
            for ast in &mut pool {
                // Mutate some trees
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if seed % 10 < 3 {
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
