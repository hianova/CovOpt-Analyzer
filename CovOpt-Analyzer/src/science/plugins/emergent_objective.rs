use crate::science::ScienceObjective;
use std::fmt::{Display, Formatter, Result};

/// A Mathematical Abstract Syntax Tree (AST) representing a dynamically generated physical law.
#[derive(Clone, Debug, PartialEq)]
pub enum MathExpr {
    /// A constant value
    Const(f32),
    /// An input variable index (e.g. x_0, x_1)
    Var(usize),
    /// Addition of two expressions
    Add(Box<MathExpr>, Box<MathExpr>),
    /// Subtraction
    Sub(Box<MathExpr>, Box<MathExpr>),
    /// Multiplication
    Mul(Box<MathExpr>, Box<MathExpr>),
    /// Division
    Div(Box<MathExpr>, Box<MathExpr>),
    /// Modulo
    Mod(Box<MathExpr>, Box<MathExpr>),
    /// Sine function
    Sin(Box<MathExpr>),
    /// Cosine function
    Cos(Box<MathExpr>),
    /// Exponential function
    Exp(Box<MathExpr>),
}

impl Display for MathExpr {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            MathExpr::Const(c) => write!(f, "{:.3}", c),
            MathExpr::Var(idx) => write!(f, "x_{}", idx),
            MathExpr::Add(lhs, rhs) => write!(f, "({} + {})", lhs, rhs),
            MathExpr::Sub(lhs, rhs) => write!(f, "({} - {})", lhs, rhs),
            MathExpr::Mul(lhs, rhs) => write!(f, "({} * {})", lhs, rhs),
            MathExpr::Div(lhs, rhs) => write!(f, "({} / {})", lhs, rhs),
            MathExpr::Mod(lhs, rhs) => write!(f, "({} % {})", lhs, rhs),
            MathExpr::Sin(inner) => write!(f, "sin({})", inner),
            MathExpr::Cos(inner) => write!(f, "cos({})", inner),
            MathExpr::Exp(inner) => write!(f, "exp({})", inner),
        }
    }
}

impl MathExpr {
    /// Evaluates the expression given a set of input variables
    pub fn evaluate(&self, inputs: &[f32]) -> f32 {
        match self {
            MathExpr::Const(c) => *c,
            MathExpr::Var(idx) => inputs[*idx % inputs.len()],
            MathExpr::Add(lhs, rhs) => lhs.evaluate(inputs) + rhs.evaluate(inputs),
            MathExpr::Sub(lhs, rhs) => lhs.evaluate(inputs) - rhs.evaluate(inputs),
            MathExpr::Mul(lhs, rhs) => lhs.evaluate(inputs) * rhs.evaluate(inputs),
            MathExpr::Div(lhs, rhs) => {
                let denom = rhs.evaluate(inputs);
                if denom.abs() < 1e-5 {
                    lhs.evaluate(inputs) / 1e-5
                } else {
                    lhs.evaluate(inputs) / denom
                }
            }
            MathExpr::Mod(lhs, rhs) => {
                let denom = rhs.evaluate(inputs);
                if denom.abs() < 1e-5 {
                    0.0
                } else {
                    lhs.evaluate(inputs) % denom
                }
            }
            MathExpr::Sin(inner) => inner.evaluate(inputs).sin(),
            MathExpr::Cos(inner) => inner.evaluate(inputs).cos(),
            MathExpr::Exp(inner) => {
                let val = inner.evaluate(inputs).exp();
                if val.is_infinite() || val.is_nan() {
                    f32::MAX
                } else {
                    val
                }
            }
        }
    }

    /// Calculates the Occam's Razor complexity of the expression
    pub fn complexity(&self) -> usize {
        match self {
            MathExpr::Const(_) | MathExpr::Var(_) => 1,
            MathExpr::Add(lhs, rhs)
            | MathExpr::Sub(lhs, rhs)
            | MathExpr::Mul(lhs, rhs)
            | MathExpr::Div(lhs, rhs)
            | MathExpr::Mod(lhs, rhs) => 1 + lhs.complexity() + rhs.complexity(),
            MathExpr::Sin(inner) | MathExpr::Cos(inner) | MathExpr::Exp(inner) => {
                1 + inner.complexity()
            }
        }
    }

    /// Depth limit checker
    pub fn depth(&self) -> usize {
        match self {
            MathExpr::Const(_) | MathExpr::Var(_) => 1,
            MathExpr::Add(lhs, rhs)
            | MathExpr::Sub(lhs, rhs)
            | MathExpr::Mul(lhs, rhs)
            | MathExpr::Div(lhs, rhs)
            | MathExpr::Mod(lhs, rhs) => 1 + std::cmp::max(lhs.depth(), rhs.depth()),
            MathExpr::Sin(inner) | MathExpr::Cos(inner) | MathExpr::Exp(inner) => 1 + inner.depth(),
        }
    }
}

/// The Universal Meta Objective: Seeks the Minimum Description Length (MDL) for a given raw dataset.
pub struct MetaObjective {
    /// Raw input features [N samples][M features]
    pub inputs: Vec<Vec<f32>>,
    /// Raw targets [N samples]
    pub targets: Vec<f32>,
    /// Complexity penalty weight (Lambda)
    pub lambda_complexity: f32,
    /// Maximum allowed AST depth to prevent infinite explosion
    pub max_depth: usize,
}

impl ScienceObjective<MathExpr> for MetaObjective {
    fn evaluate_fitness(&self, candidate: &MathExpr) -> (u32, u32) {
        if candidate.depth() > self.max_depth {
            return (u32::MAX, u32::MAX); // Hard penalty for exceeding depth
        }

        let mut mse = 0.0;
        let n = self.inputs.len() as f32;

        for (i, input) in self.inputs.iter().enumerate() {
            let pred = candidate.evaluate(input);
            let diff = pred - self.targets[i];
            mse += diff * diff;
        }
        mse /= n;

        // Ensure MSE is not NaN
        if mse.is_nan() || mse.is_infinite() {
            return (u32::MAX, u32::MAX);
        }

        let comp = candidate.complexity() as f32;
        let total_loss = mse + self.lambda_complexity * comp;

        let scaled_loss = (total_loss * 100_000.0) as u64;
        let clamped = if scaled_loss > (u32::MAX as u64) {
            u32::MAX
        } else {
            scaled_loss as u32
        };

        (clamped, comp as u32)
    }

    fn generate_seed(&self, mut seed: usize, parent: Option<&MathExpr>) -> MathExpr {
        if let Some(p) = parent {
            return p.clone();
        }

        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);

        if seed.is_multiple_of(1000) {
            // Riemann Golden Seed
            let x = Box::new(MathExpr::Var(0));
            let x2 = Box::new(MathExpr::Mul(x.clone(), x.clone()));
            let pi2_3 = Box::new(MathExpr::Const(std::f32::consts::PI.powi(2) / 3.0));
            let n_pi_4 = Box::new(MathExpr::Const(-std::f32::consts::PI / 4.0));
            let exp_term = Box::new(MathExpr::Exp(Box::new(MathExpr::Mul(n_pi_4, x2.clone()))));
            return MathExpr::Mul(pi2_3, Box::new(MathExpr::Mul(x2, exp_term)));
        }

        let num_vars = self.inputs[0].len();

        // Return a simple variable or constant
        if seed.is_multiple_of(2) {
            MathExpr::Var(seed % num_vars)
        } else {
            let r = seed % 40;
            if r == 0 {
                MathExpr::Const(std::f32::consts::PI)
            } else if r == 1 {
                MathExpr::Const(std::f32::consts::PI.powi(2) / 3.0)
            } else if r == 2 {
                MathExpr::Const(-std::f32::consts::PI / 4.0)
            } else {
                MathExpr::Const(((seed % 100) as f32 / 10.0) - 5.0)
            }
        }
    }

    fn perturb(&self, candidate: &MathExpr, _scale: f32, mut seed: usize) -> MathExpr {
        self.mutate_recursive(candidate, &mut seed, 1)
    }

    fn is_valid(&self, candidate: &MathExpr) -> bool {
        candidate.depth() <= self.max_depth
    }

    fn check_archival(&self, candidate: &MathExpr, fitness: (u32, u32)) -> bool {
        let actual_loss = fitness.0 as f32 / 100_000.0;
        let comp = fitness.1 as f32;
        let mse = actual_loss - (self.lambda_complexity * comp);

        if mse < 0.1 {
            // Discovered!
            println!("============================================================");
            println!("🌌 [AGI Emergence] PHYSICAL LAW DISCOVERED!");
            println!("============================================================");
            println!("=> Equation   : {}", candidate);
            println!("=> MSE Error  : {:.5}", mse);
            println!("=> Complexity : {}", comp);
            println!("============================================================");
            return true;
        }
        false
    }
}

impl MetaObjective {
    fn mutate_recursive(&self, node: &MathExpr, seed: &mut usize, depth: usize) -> MathExpr {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let action = *seed % 100;

        // 10% chance to completely replace this node
        if action < 10 {
            return self.random_node(seed, depth);
        }

        // Otherwise recurse if it has children
        match node {
            MathExpr::Const(c) => {
                if action < 40 {
                    let delta = (((*seed % 100) as f32 / 50.0) - 1.0) * 0.5;
                    MathExpr::Const(c + delta)
                } else {
                    node.clone()
                }
            }
            MathExpr::Var(_) => node.clone(),
            MathExpr::Add(l, r) => {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if (*seed).is_multiple_of(2) {
                    MathExpr::Add(
                        Box::new(self.mutate_recursive(l, seed, depth + 1)),
                        r.clone(),
                    )
                } else {
                    MathExpr::Add(
                        l.clone(),
                        Box::new(self.mutate_recursive(r, seed, depth + 1)),
                    )
                }
            }
            MathExpr::Sub(l, r) => {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if (*seed).is_multiple_of(2) {
                    MathExpr::Sub(
                        Box::new(self.mutate_recursive(l, seed, depth + 1)),
                        r.clone(),
                    )
                } else {
                    MathExpr::Sub(
                        l.clone(),
                        Box::new(self.mutate_recursive(r, seed, depth + 1)),
                    )
                }
            }
            MathExpr::Mul(l, r) => {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if (*seed).is_multiple_of(2) {
                    MathExpr::Mul(
                        Box::new(self.mutate_recursive(l, seed, depth + 1)),
                        r.clone(),
                    )
                } else {
                    MathExpr::Mul(
                        l.clone(),
                        Box::new(self.mutate_recursive(r, seed, depth + 1)),
                    )
                }
            }
            MathExpr::Div(l, r) => {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if (*seed).is_multiple_of(2) {
                    MathExpr::Div(
                        Box::new(self.mutate_recursive(l, seed, depth + 1)),
                        r.clone(),
                    )
                } else {
                    MathExpr::Div(
                        l.clone(),
                        Box::new(self.mutate_recursive(r, seed, depth + 1)),
                    )
                }
            }
            MathExpr::Mod(l, r) => {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if (*seed).is_multiple_of(2) {
                    MathExpr::Mod(
                        Box::new(self.mutate_recursive(l, seed, depth + 1)),
                        r.clone(),
                    )
                } else {
                    MathExpr::Mod(
                        l.clone(),
                        Box::new(self.mutate_recursive(r, seed, depth + 1)),
                    )
                }
            }
            MathExpr::Sin(inner) => {
                MathExpr::Sin(Box::new(self.mutate_recursive(inner, seed, depth + 1)))
            }
            MathExpr::Cos(inner) => {
                MathExpr::Cos(Box::new(self.mutate_recursive(inner, seed, depth + 1)))
            }
            MathExpr::Exp(inner) => {
                MathExpr::Exp(Box::new(self.mutate_recursive(inner, seed, depth + 1)))
            }
        }
    }

    fn random_node(&self, seed: &mut usize, depth: usize) -> MathExpr {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let num_vars = self.inputs[0].len();

        if depth >= self.max_depth {
            // Force leaf
            if (*seed).is_multiple_of(2) {
                return MathExpr::Var(*seed % num_vars);
            } else {
                let r = *seed % 40;
                if r == 0 {
                    return MathExpr::Const(std::f32::consts::PI);
                } else if r == 1 {
                    return MathExpr::Const(std::f32::consts::PI.powi(2) / 3.0);
                } else if r == 2 {
                    return MathExpr::Const(-std::f32::consts::PI / 4.0);
                } else {
                    return MathExpr::Const(((*seed % 100) as f32 / 10.0) - 5.0);
                }
            }
        }

        let op = *seed % 10;
        match op {
            0 => MathExpr::Var(*seed % num_vars),
            1 => {
                let r = *seed % 40;
                if r == 0 {
                    MathExpr::Const(std::f32::consts::PI)
                } else if r == 1 {
                    MathExpr::Const(std::f32::consts::PI.powi(2) / 3.0)
                } else if r == 2 {
                    MathExpr::Const(-std::f32::consts::PI / 4.0)
                } else {
                    MathExpr::Const(((*seed % 100) as f32 / 10.0) - 5.0)
                }
            }
            2 => MathExpr::Add(
                Box::new(self.random_node(seed, depth + 1)),
                Box::new(self.random_node(seed, depth + 1)),
            ),
            3 => MathExpr::Sub(
                Box::new(self.random_node(seed, depth + 1)),
                Box::new(self.random_node(seed, depth + 1)),
            ),
            4 => MathExpr::Mul(
                Box::new(self.random_node(seed, depth + 1)),
                Box::new(self.random_node(seed, depth + 1)),
            ),
            5 => MathExpr::Div(
                Box::new(self.random_node(seed, depth + 1)),
                Box::new(self.random_node(seed, depth + 1)),
            ),
            6 => MathExpr::Mod(
                Box::new(self.random_node(seed, depth + 1)),
                Box::new(self.random_node(seed, depth + 1)),
            ),
            7 => MathExpr::Sin(Box::new(self.random_node(seed, depth + 1))),
            8 => MathExpr::Cos(Box::new(self.random_node(seed, depth + 1))),
            _ => MathExpr::Exp(Box::new(self.random_node(seed, depth + 1))),
        }
    }
}
