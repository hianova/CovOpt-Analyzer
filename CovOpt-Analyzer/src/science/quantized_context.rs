#[derive(Clone, Debug, PartialEq)]
pub struct QuantizedArray<const N: usize, const SCALE: i32> {
    pub raw_ints: [i32; N],
}

impl<const N: usize, const SCALE: i32> Default for QuantizedArray<N, SCALE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize, const SCALE: i32> QuantizedArray<N, SCALE> {
    pub fn new() -> Self {
        Self { raw_ints: [0; N] }
    }

    pub fn to_f32_array(&self) -> [f32; N] {
        let mut out = [0.0; N];
        for (i, val) in self.raw_ints.iter().enumerate() {
            out[i] = *val as f32 / SCALE as f32;
        }
        out
    }

    pub fn generate_seed(mut seed: usize) -> Self {
        let mut out = Self::new();
        for i in 0..N {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let val = (seed % (SCALE.unsigned_abs() as usize * 2)) as i32 - SCALE.abs();
            out.raw_ints[i] = val;
        }
        out
    }

    pub fn perturb(&self, scale_f32: f32, mut seed: usize) -> Self {
        let mut child = self.clone();
        let num_mutations = (scale_f32 * (N as f32 / 5.0)).max(1.0).ceil() as usize;

        for _ in 0..num_mutations {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let idx = seed % N;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);

            let max_step = (SCALE as f32 * scale_f32).round() as i32;
            let max_s = max_step.max(1);
            let step = (seed % (2 * max_s as usize + 1)) as i32 - max_s;

            child.raw_ints[idx] = child.raw_ints[idx].saturating_add(step);
        }
        child
    }
}
