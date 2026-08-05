//! ChaosState Engine
//! A neuro-symbolic pure static math engine for simulating state transitions.

/// Lightweight, zero-allocation Xorshift32 PRNG.
pub struct RngState {
    pub seed: u32,
}

impl RngState {
    pub fn new(seed: u32) -> Self {
        Self {
            seed: if seed == 0 { 0x12345678 } else { seed },
        }
    }

    /// Generates a uniform f32 in range [0, 1)
    pub fn next_f32(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as f32) / (u32::MAX as f32)
    }

    pub fn next_zipf(&mut self, tweak: &MicroTweak) -> f32 {
        let mut u = self.next_f32();
        if u == 0.0 {
            u = 0.000001; // Avoid division by zero
        }

        let s_minus_1 = tweak.s_exponent - 1.0;
        let s_minus_1 = if s_minus_1 < 0.01 { 0.01 } else { s_minus_1 };

        let p = -1.0 / s_minus_1;
        let val = u.powf(p);

        // Prevent massive mathematical overflow during extreme chaos
        if val.is_infinite() || val > 10000.0 {
            10000.0
        } else {
            val
        }
    }
}

/// Tweakable parameters adjusting the micro-randomness of the system.
pub struct MicroTweak {
    pub s_exponent: f32,
    pub max_elements: u32,
}

/// Adaptive Interface for dynamical systems to auto-tune Chaos.
pub trait StagnationFeedback {
    /// Returns the current stagnation gradient (e.g. 0.0 means moving fast, 1.0 means completely stuck).
    fn current_gradient(&self) -> f32;
}

/// A node representing the system's "probability cloud" across N branches and D dimensions.
#[derive(Clone, Copy)]
pub struct ChaosState<const N: usize, const D: usize> {
    pub macro_weights: [f32; N],
    pub base_values: [f32; D],
}

impl<const N: usize, const D: usize> ChaosState<N, D> {
    pub fn new(initial_values: [f32; D]) -> Self {
        let mut weights = [0.0; N];
        if N > 0 {
            weights[0] = 1.0;
        } // Default to first branch taking full probability
        Self {
            macro_weights: weights,
            base_values: initial_values,
        }
    }

    /// Adapts the tweak parameter based on external stagnation feedback.
    /// If the system is highly stagnant (gradient approaches 1.0), it decreases `s_exponent` to induce chaos.
    pub fn adapt_tweak(&self, tweak: &mut MicroTweak, feedback: &impl StagnationFeedback) {
        let grad = feedback.current_gradient();
        // Lower s -> more chaos. Bound between 1.1 (Extreme chaos) and 3.0 (Gaussian-like).
        let target_s = 3.0 - (grad * 1.9);
        tweak.s_exponent = target_s.clamp(1.1, 3.0);
    }

    /// Serializes the ChaosState into a raw byte vector for fast temporal storage.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((N + D) * 4);
        for w in self.macro_weights.iter() {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        for b in self.base_values.iter() {
            bytes.extend_from_slice(&b.to_le_bytes());
        }
        bytes
    }

    /// Deserializes the ChaosState from a raw byte slice.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < (N + D) * 4 {
            return None;
        }
        let mut macro_weights = [0.0; N];
        let mut base_values = [0.0; D];

        let mut offset = 0;
        for val in &mut macro_weights {
            let chunk = data.get(offset..offset + 4)?;
            *val = f32::from_le_bytes(chunk.try_into().ok()?);
            offset += 4;
        }
        for val in &mut base_values {
            let chunk = data.get(offset..offset + 4)?;
            *val = f32::from_le_bytes(chunk.try_into().ok()?);
            offset += 4;
        }

        Some(Self {
            macro_weights,
            base_values,
        })
    }
}

/// Normalizes a float array in-place so that its elements sum to 1.0.
pub fn normalize_weights<const N: usize>(weights: &mut [f32; N]) {
    let sum: f32 = weights.iter().sum();
    if sum > 0.0 {
        for w in weights.iter_mut() {
            *w /= sum;
        }
    } else {
        let equiprob = 1.0 / (N as f32);
        for w in weights.iter_mut() {
            *w = equiprob;
        }
    }
}

/// Progresses the N-Dimensional ChaosState without a hook.
pub fn step_forward_nd<const N: usize, const D: usize>(
    current: &ChaosState<N, D>,
    tweak: &MicroTweak,
    rng: &mut RngState,
) -> ChaosState<N, D> {
    step_forward_nd_with_hook(current, tweak, rng, |_, _| {})
}

/// Progresses the N-Dimensional ChaosState with a Black Swan event callback.
pub fn step_forward_nd_with_hook<const N: usize, const D: usize, F>(
    current: &ChaosState<N, D>,
    tweak: &MicroTweak,
    rng: &mut RngState,
    mut on_black_swan: F,
) -> ChaosState<N, D>
where
    F: FnMut(usize, f32), // Passes dimension index and extreme value impact
{
    let mut next_weights = [0.0; N];
    let mut next_bases = current.base_values;

    for (i, w_next) in next_weights.iter_mut().enumerate() {
        let w = current.macro_weights[i];

        let r = rng.next_zipf(tweak);
        let direction = if rng.next_f32() > 0.5 { 1.0 } else { -1.0 };
        let impact = w * r * direction;

        // N-Dimensional Multi-variate Levy Flight Jump
        for (dim, val) in next_bases.iter_mut().enumerate() {
            // Apply a slight dimension-specific variance multiplier
            let dim_variance = (rng.next_f32() * 2.0) - 1.0;
            let final_impact = impact * dim_variance;

            *val += final_impact;

            // Trigger Black Swan Event Hook if impact is extremely massive (e.g. > 10.0 or < -10.0)
            if final_impact.abs() > 10.0 {
                on_black_swan(dim, final_impact);
            }
        }

        let volatility = impact.abs();
        *w_next = w + volatility * 0.01;
    }

    normalize_weights(&mut next_weights);

    ChaosState {
        macro_weights: next_weights,
        base_values: next_bases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyFeedback {
        grad: f32,
    }
    impl StagnationFeedback for DummyFeedback {
        fn current_gradient(&self) -> f32 {
            self.grad
        }
    }

    #[test]
    fn test_rng_state() {
        let mut rng = RngState::new(42);
        let val1 = rng.next_f32();
        let val2 = rng.next_f32();
        assert!((0.0..1.0).contains(&val1));
        assert_ne!(val1, val2);

        let tweak = MicroTweak {
            s_exponent: 1.5,
            max_elements: 1000,
        };
        let z = rng.next_zipf(&tweak);
        assert!(z > 0.0);
    }

    #[test]
    fn test_chaos_state_serialization() {
        let state = ChaosState::<2, 3>::new([1.0, 2.0, 3.0]);
        let bytes = state.to_bytes();
        let state2 = ChaosState::<2, 3>::from_bytes(&bytes).unwrap();
        assert_eq!(state.base_values, state2.base_values);
        assert_eq!(state.macro_weights, state2.macro_weights);

        assert!(ChaosState::<2, 3>::from_bytes(&[0; 4]).is_none());
    }

    #[test]
    fn test_adapt_tweak() {
        let state = ChaosState::<1, 1>::new([0.0]);
        let mut tweak = MicroTweak {
            s_exponent: 2.0,
            max_elements: 1000,
        };
        state.adapt_tweak(&mut tweak, &DummyFeedback { grad: 1.0 });
        // target_s = 3.0 - 1.9 = 1.1
        assert!((tweak.s_exponent - 1.1).abs() < 1e-4);
    }

    #[test]
    fn test_step_forward() {
        let current = ChaosState::<2, 1>::new([0.0]);
        let tweak = MicroTweak {
            s_exponent: 2.0,
            max_elements: 1000,
        };
        let mut rng = RngState::new(123);

        let next = step_forward_nd(&current, &tweak, &mut rng);
        assert_ne!(current.base_values, next.base_values);

        let sum: f32 = next.macro_weights.iter().sum();
        assert!((sum - 1.0).abs() < 1e-4);
    }
}
