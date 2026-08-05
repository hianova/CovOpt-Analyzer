use crate::science::ScienceObjective;
use cdDB::{DualCacheFF, dualcache_ff};
use std::sync::Arc;
use std::time::Instant;

pub type MotifCache = DualCacheFF<u64, Arc<(usize, AsicCircuit)>, 1024, 2048, 4096, 7168>;

pub fn create_default_motif_cache() -> Arc<MotifCache> {
    Arc::new(MotifCache::new())
}

pub struct TruthTableBuilder {
    pub input_bits: usize,
    pub output_bits: usize,
}

impl TruthTableBuilder {
    pub fn new(input_bits: usize, output_bits: usize) -> Self {
        Self {
            input_bits,
            output_bits,
        }
    }

    /// `generator` receives the row index (0 to 2^input_bits - 1)
    /// and returns the expected output as a u64.
    pub fn generate<F>(self, generator: F) -> Vec<(Vec<bool>, Vec<bool>)>
    where
        F: Fn(u64) -> u64,
    {
        let total_rows = 1 << self.input_bits;
        let mut table = Vec::with_capacity(total_rows);

        for row in 0..total_rows {
            let mut inputs = vec![false; self.input_bits];
            for b in 0..self.input_bits {
                inputs[self.input_bits - 1 - b] = (row >> b) & 1 == 1;
            }

            let out_val = generator(row as u64);
            let mut outputs = vec![false; self.output_bits];
            for b in 0..self.output_bits {
                outputs[self.output_bits - 1 - b] = (out_val >> b) & 1 == 1;
            }
            table.push((inputs, outputs));
        }

        table
    }
}
use rayon::prelude::*;
use std::cell::RefCell;

type EvalBufferTuple = (Vec<[u64; 4]>, Vec<u32>, Vec<bool>);

thread_local! {
    static EVAL_BUFFER: RefCell<EvalBufferTuple> = RefCell::new((
        Vec::with_capacity(3500),
        Vec::with_capacity(3500),
        Vec::with_capacity(3500),
    ));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GateOp {
    And = 0,
    Or = 1,
    Xor = 2,
    Nand = 3,
    Nor = 4,
    Xnor = 5,
    Not = 6, // Unary operation (only uses left input)
}

impl GateOp {
    pub fn from_u8(val: u8) -> Self {
        match val % 7 {
            0 => GateOp::And,
            1 => GateOp::Or,
            2 => GateOp::Xor,
            3 => GateOp::Nand,
            4 => GateOp::Nor,
            5 => GateOp::Xnor,
            _ => GateOp::Not,
        }
    }

    pub fn evaluate(&self, a: f32, b: f32) -> f32 {
        match self {
            GateOp::And => a * b,
            GateOp::Or => a + b - (a * b),
            GateOp::Xor => a + b - 2.0 * (a * b),
            GateOp::Nand => 1.0 - (a * b),
            GateOp::Nor => 1.0 - (a + b - (a * b)),
            GateOp::Xnor => 1.0 - (a + b - 2.0 * (a * b)),
            GateOp::Not => 1.0 - a,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Gate {
    pub left: u16,
    pub right: u16,
    pub op: GateOp,
}

#[derive(Clone, Debug)]
pub struct AsicCircuit {
    pub gates: Vec<Gate>,
    pub output_map: Vec<u16>,
    pub affine_mask: [u8; 8],
    pub affine_const: u8,
}

pub struct BooleanRelaxationObjective {
    pub num_inputs: usize,
    pub num_outputs: usize,
    pub input_wires: Vec<[u64; 4]>,
    pub target_wires: Vec<[u64; 4]>,
    pub start_time: Instant,
    pub max_gates: usize,
    pub llm_seed: Option<AsicCircuit>,
    pub cached_motifs: Arc<MotifCache>, // (num_inputs, motif)
}

impl BooleanRelaxationObjective {
    pub fn new(
        num_inputs: usize,
        num_outputs: usize,
        target_truth_table: Vec<(Vec<bool>, Vec<bool>)>,
        max_gates: usize,
        cached_motifs: Arc<MotifCache>,
    ) -> Self {
        assert!(
            target_truth_table.len() <= 256,
            "Bitwise parallel overdrive currently supports max 256 rows"
        );

        let mut input_wires = vec![[0u64; 4]; num_inputs];
        let mut target_wires = vec![[0u64; 4]; num_outputs];

        for (row_idx, (inputs, outputs)) in target_truth_table.iter().enumerate() {
            let chunk = row_idx / 64;
            let bit_shift = row_idx % 64;

            for (i, &val) in inputs.iter().enumerate() {
                if val {
                    input_wires[i][chunk] |= 1 << bit_shift;
                }
            }

            for (i, &val) in outputs.iter().enumerate() {
                if val {
                    target_wires[i][chunk] |= 1 << bit_shift;
                }
            }
        }

        Self {
            num_inputs,
            num_outputs,
            input_wires,
            target_wires,
            start_time: Instant::now(),
            max_gates,
            llm_seed: None,
            cached_motifs,
        }
    }

    pub fn clamp_circuit(&self, circuit: &mut AsicCircuit) {
        let max_idx = self.num_inputs + circuit.gates.len();
        for (i, gate) in circuit.gates.iter_mut().enumerate() {
            let valid_max = self.num_inputs + i;
            if gate.left as usize >= valid_max {
                gate.left = (valid_max.saturating_sub(1)) as u16;
            }
            if gate.right as usize >= valid_max {
                gate.right = (valid_max.saturating_sub(1)) as u16;
            }
        }
        for out in &mut circuit.output_map {
            if *out as usize >= max_idx {
                *out = (max_idx.saturating_sub(1)) as u16;
            }
        }
    }
}

impl ScienceObjective<AsicCircuit> for BooleanRelaxationObjective {
    fn denoise_step(&self, candidate: &mut AsicCircuit, noise_level: f32, seed: usize) {
        // Step 1: Always ensure structural bounds are correct
        self.clamp_circuit(candidate);

        // Step 2: Heuristic structural denoising based on noise_level
        if noise_level < 0.5 {
            // As noise lowers, randomly prune useless consecutive operations
            // (Simulated by replacing gates with simpler ones or wiring directly)
            let mut rng = seed;
            for gate in &mut candidate.gates {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                if gate.op == GateOp::Not || gate.op == GateOp::Xor {
                    // 10% chance to simplify Xor to And, Not to wire (left=right) if noise is low
                    if (rng % 100) < 10 {
                        gate.op = GateOp::And;
                    }
                }
            }
        }
    }

    fn evaluate_fitness_batch(&self, candidates: &[AsicCircuit], out_fitness: &mut [(u32, u32)]) {
        let batch_size = candidates.len();
        if batch_size == 0 {
            return;
        }

        out_fitness
            .iter_mut()
            .enumerate()
            .for_each(|(idx, fit)| {
                *fit = self.evaluate_fitness(&candidates[idx]);
            });
    }

    fn evaluate_fitness(&self, candidate: &AsicCircuit) -> (u32, u32) {
        EVAL_BUFFER.with(|buf| {
            let mut b = buf.borrow_mut();
            let total_len = self.num_inputs + candidate.gates.len();
            let b_ref = &mut *b;

            if b_ref.0.len() < total_len {
                let new_cap = total_len.max(4096);
                b_ref.0.resize(new_cap, [0u64; 4]);
                b_ref.1.resize(new_cap, 0u32);
                b_ref.2.resize(new_cap, false);
            }

            let wires = &mut b_ref.0[..total_len];
            let depths = &mut b_ref.1[..total_len];
            let active_wires = &mut b_ref.2[..total_len];

            active_wires.fill(false);

            wires[..self.num_inputs].copy_from_slice(&self.input_wires[..self.num_inputs]);

            for (i, gate) in candidate.gates.iter().enumerate() {
                let l = wires[gate.left as usize];
                let r = wires[gate.right as usize];
                let out = &mut wires[self.num_inputs + i];

                let l_depth = depths[gate.left as usize];
                let r_depth = depths[gate.right as usize];
                let is_nonlinear = matches!(
                    gate.op,
                    GateOp::And | GateOp::Nand | GateOp::Or | GateOp::Nor
                );
                depths[self.num_inputs + i] =
                    l_depth.max(r_depth) + if is_nonlinear { 1 } else { 0 };

                match gate.op {
                    GateOp::And => {
                        for k in 0..4 {
                            out[k] = l[k] & r[k];
                        }
                    }
                    GateOp::Or => {
                        for k in 0..4 {
                            out[k] = l[k] | r[k];
                        }
                    }
                    GateOp::Xor => {
                        for k in 0..4 {
                            out[k] = l[k] ^ r[k];
                        }
                    }
                    GateOp::Nand => {
                        for k in 0..4 {
                            out[k] = !(l[k] & r[k]);
                        }
                    }
                    GateOp::Nor => {
                        for k in 0..4 {
                            out[k] = !(l[k] | r[k]);
                        }
                    }
                    GateOp::Xnor => {
                        for k in 0..4 {
                            out[k] = !(l[k] ^ r[k]);
                        }
                    }
                    GateOp::Not => {
                        for k in 0..4 {
                            out[k] = !l[k];
                        }
                    }
                }
            }

            let mut total_incorrect = 0;
            for j in 0..self.num_outputs {
                let mask = candidate.affine_mask[j];
                let mut actual_output = [0u64; 4];

                if (candidate.affine_const >> j) & 1 == 1 {
                    actual_output = [!0u64; 4];
                }

                let mut current_mask = mask;
                while current_mask != 0 {
                    let bit = current_mask.trailing_zeros();
                    if bit < candidate.output_map.len() as u32 {
                        let wire_idx = candidate.output_map[bit as usize];
                        let wire_val = wires[wire_idx as usize];
                        for k in 0..4 {
                            actual_output[k] ^= wire_val[k];
                        }
                    }
                    current_mask &= current_mask - 1; // Clear lowest set bit
                }

                let target = self.target_wires[j];
                for k in 0..4 {
                    total_incorrect += (actual_output[k] ^ target[k]).count_ones();
                }
            }

            for j in 0..self.num_outputs {
                let mask = candidate.affine_mask[j];
                let mut current_mask = mask;
                while current_mask != 0 {
                    let bit = current_mask.trailing_zeros();
                    if bit < candidate.output_map.len() as u32 {
                        let out_pin = candidate.output_map[bit as usize];
                        active_wires[out_pin as usize] = true;
                    }
                    current_mask &= current_mask - 1;
                }
            }

            let mut active_gate_count = 0;
            let mut fan_out = vec![0u32; total_len];
            let mut max_fan_out_penalty = 0;

            for (i, gate) in candidate.gates.iter().enumerate().rev() {
                let gate_wire_idx = self.num_inputs + i;
                if active_wires[gate_wire_idx] {
                    active_gate_count += 1;
                    active_wires[gate.left as usize] = true;
                    fan_out[gate.left as usize] += 1;
                    if fan_out[gate.left as usize] > 4 {
                        max_fan_out_penalty += 2;
                    }

                    if gate.op != GateOp::Not {
                        active_wires[gate.right as usize] = true;
                        fan_out[gate.right as usize] += 1;
                        if fan_out[gate.right as usize] > 4 {
                            max_fan_out_penalty += 2;
                        }
                    }
                }
            }

            // Allow "Junk DNA" (neutral mutations) to survive when searching for functional correctness,
            // but add a minor penalty for extreme bloat and Fan-Out physical limits.
            let reported_gates = if total_incorrect > 0 {
                // 1 penalty point per 10 gates to avoid infinite memory bloat
                (active_gate_count as u32 / 10) + max_fan_out_penalty
            } else {
                active_gate_count as u32 + max_fan_out_penalty
            };

            (total_incorrect, reported_gates)
        })
    }

    fn generate_seed(&self, mut seed: usize, parent: Option<&AsicCircuit>) -> AsicCircuit {
        if let Some(p) = parent {
            // Keep mutation small for elites (e.g. 0.01% to 1% -> 1 to 15 mutations)
            let scale = ((seed % 100) as f32 / 10000.0) + 0.0001;
            return self.perturb(p, scale, seed);
        }

        if let Some(llm_seed) = &self.llm_seed
            && seed % 100 < 5
        {
            // Keep mutation small for LLM seed (e.g. 0% to 5% -> 0 to 75 mutations)
            let scale = (seed % 100) as f32 / 2000.0;
            return self.perturb(llm_seed, scale, seed);
        }

        let mut gates = Vec::with_capacity(self.max_gates);
        for i in 0..self.max_gates {
            let max_idx = (self.num_inputs + i) as u16;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let left = (seed % max_idx as usize) as u16;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let right = (seed % max_idx as usize) as u16;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let op = GateOp::from_u8((seed % 7) as u8);
            gates.push(Gate { left, right, op });
        }

        let mut output_map = Vec::with_capacity(self.num_outputs);
        let max_wire_idx = (self.num_inputs + self.max_gates) as u16;
        for _ in 0..self.num_outputs {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            output_map.push((seed % max_wire_idx as usize) as u16);
        }

        let mut affine_mask = [0u8; 8];
        for (i, mask) in affine_mask
            .iter_mut()
            .enumerate()
            .take(self.num_outputs.min(8))
        {
            *mask = 1 << i;
        }

        AsicCircuit {
            gates,
            output_map,
            affine_mask,
            affine_const: 0,
        }
    }

    fn perturb(&self, candidate: &AsicCircuit, scale: f32, mut seed: usize) -> AsicCircuit {
        let mut child = candidate.clone();
        let max_mutations = (self.max_gates as f32 / 2.0).max(1.0);
        let num_mutations = (scale * max_mutations).ceil() as usize;

        for _ in 0..num_mutations {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let mut_type = seed % 100;

            if mut_type < 5 {
                let motif_idx = (seed % 5000) as u64;
                thread_local! {
                    static MOTIF_HANDLE: std::cell::RefCell<Option<dualcache_ff::component::tls::TlsHandle>> = const { std::cell::RefCell::new(None) };
                }
                let motif_opt = MOTIF_HANDLE.with(|h| {
                    let mut h_ref = h.borrow_mut();
                    if h_ref.is_none() {
                        *h_ref = Some(self.cached_motifs.register_thread());
                    }
                    self.cached_motifs.get(&motif_idx, h_ref.as_ref().unwrap())
                });

                if let Some(motif) = motif_opt {
                    let (num_motif_inputs, m_circuit) = &*motif;
                    let num_motif_gates = m_circuit.gates.len();

                    if *num_motif_inputs <= self.max_gates && !m_circuit.gates.is_empty() {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        let inject_pos =
                            seed % self.max_gates.saturating_sub(num_motif_gates).max(1);

                        let mut mapping = vec![0u16; *num_motif_inputs + num_motif_gates];
                        for val in mapping.iter_mut().take(*num_motif_inputs) {
                            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                            *val = (seed % (self.num_inputs + inject_pos)) as u16;
                        }

                        for (i, gate) in m_circuit.gates.iter().enumerate() {
                            if inject_pos + i < self.max_gates {
                                let mapped_l = mapping[gate.left as usize];
                                let mapped_r = mapping[gate.right as usize];
                                child.gates[inject_pos + i] = Gate {
                                    left: mapped_l,
                                    right: mapped_r,
                                    op: gate.op,
                                };
                                mapping[*num_motif_inputs + i] =
                                    (self.num_inputs + inject_pos + i) as u16;
                            }
                        }
                    }
                }
            } else if mut_type < 15 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let row = seed % self.num_outputs;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let bit = seed % 8;
                child.affine_mask[row] ^= 1 << bit;
            } else if mut_type < 20 {
                // Mutate affine_const (5%)
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let row = seed % self.num_outputs;
                child.affine_const ^= 1 << row;
            } else if mut_type < 70 {
                // Mutate logic gate (50%)
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let gate_idx = seed % child.gates.len();
                let max_idx = (self.num_inputs + gate_idx) as u16;

                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let part = seed % 3;

                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if part == 0 {
                    child.gates[gate_idx].left = (seed % max_idx as usize) as u16;
                } else if part == 1 {
                    child.gates[gate_idx].right = (seed % max_idx as usize) as u16;
                } else {
                    child.gates[gate_idx].op = GateOp::from_u8((seed % 7) as u8);
                }
            } else if mut_type < 80 {
                // Insert Gate (10%)
                if child.gates.len() < (self.max_gates as f32 * 1.5) as usize {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let inject_pos = seed % (child.gates.len() + 1);

                    let valid_max = self.num_inputs + inject_pos;
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let left = (seed % valid_max.max(1)) as u16;
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let right = (seed % valid_max.max(1)) as u16;
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let op = GateOp::from_u8((seed % 7) as u8);

                    child.gates.insert(inject_pos, Gate { left, right, op });

                    // Remap downstream references
                    for i in (inject_pos + 1)..child.gates.len() {
                        if child.gates[i].left as usize >= self.num_inputs + inject_pos {
                            child.gates[i].left += 1;
                        }
                        if child.gates[i].right as usize >= self.num_inputs + inject_pos {
                            child.gates[i].right += 1;
                        }
                    }
                    for out in &mut child.output_map {
                        if *out as usize >= self.num_inputs + inject_pos {
                            *out += 1;
                        }
                    }
                }
            } else if mut_type < 90 {
                // Remove Gate (10%)
                if child.gates.len() > 10 {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let remove_pos = seed % child.gates.len();
                    child.gates.remove(remove_pos);

                    // Remap downstream references
                    for i in remove_pos..child.gates.len() {
                        if child.gates[i].left as usize > self.num_inputs + remove_pos {
                            child.gates[i].left -= 1;
                        } else if child.gates[i].left as usize == self.num_inputs + remove_pos {
                            child.gates[i].left = self.num_inputs as u16; // tie to input 0
                        }

                        if child.gates[i].right as usize > self.num_inputs + remove_pos {
                            child.gates[i].right -= 1;
                        } else if child.gates[i].right as usize == self.num_inputs + remove_pos {
                            child.gates[i].right = self.num_inputs as u16;
                        }
                    }
                    for out in &mut child.output_map {
                        if *out as usize > self.num_inputs + remove_pos {
                            *out -= 1;
                        } else if *out as usize == self.num_inputs + remove_pos {
                            *out = self.num_inputs as u16;
                        }
                    }
                }
            } else {
                // Mutate output mapping (10%)
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let out_idx = seed % self.num_outputs;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let max_wire_idx = (self.num_inputs + child.gates.len()) as u16;
                child.output_map[out_idx] = (seed % max_wire_idx as usize) as u16;
            }
        }
        child
    }

    fn is_valid(&self, _candidate: &AsicCircuit) -> bool {
        true
    }

    fn check_archival(&self, candidate: &AsicCircuit, fitness: (u32, u32)) -> bool {
        let (total_incorrect, active_gate_count) = fitness;

        if total_incorrect == 0 && active_gate_count <= 112 {
            println!("============================================================");
            println!("🏆 🏆 🏆 🏆 WORLD RECORD ACHIEVED! 🏆 🏆 🏆 🏆");
            println!("  0 Incorrect Bits, <= 112 Gates! Target hit.");
            println!("============================================================");
            println!(
                "=> Time Elapsed   : {:.3} seconds",
                self.start_time.elapsed().as_secs_f32()
            );
            println!("=> Assembly Index : {} Active Gates", active_gate_count);

            println!("[Synthesized Netlist]");
            let mut active_wires = vec![false; self.num_inputs + candidate.gates.len()];
            for &out_pin in &candidate.output_map {
                active_wires[out_pin as usize] = true;
            }
            for (i, gate) in candidate.gates.iter().enumerate().rev() {
                let gate_wire_idx = self.num_inputs + i;
                if active_wires[gate_wire_idx] {
                    active_wires[gate.left as usize] = true;
                    if gate.op != GateOp::Not {
                        active_wires[gate.right as usize] = true;
                    }
                }
            }

            for (i, gate) in candidate.gates.iter().enumerate() {
                let gate_wire_idx = self.num_inputs + i;
                if !active_wires[gate_wire_idx] {
                    continue;
                }

                let get_name = |idx: u16| -> String {
                    if idx < self.num_inputs as u16 {
                        format!("INP_{}", idx)
                    } else {
                        format!("W_{}", idx)
                    }
                };

                let lhs = get_name(gate.left);
                let rhs = get_name(gate.right);
                let out = get_name(gate_wire_idx as u16);

                match gate.op {
                    GateOp::Not => println!("  {} = NOT {}", out, lhs),
                    _ => println!("  {} = {} {:?} {}", out, lhs, gate.op, rhs),
                }
            }

            println!("[Output Mapping]");
            for (j, &out_pin) in candidate.output_map.iter().enumerate() {
                if out_pin < self.num_inputs as u16 {
                    println!("  OUT_{} = INP_{}", j, out_pin);
                } else {
                    println!("  OUT_{} = W_{}", j, out_pin);
                }
            }
            println!("============================================================");

            // 安全歸檔 (Persistent Archival with fsync)
            use std::fs::OpenOptions;
            use std::io::Write;
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open("aes_sbox_record.txt")
            {
                let _ = writeln!(
                    file,
                    "============================================================"
                );
                let _ = writeln!(file, "🚨 PERFECT ASIC LOGIC CIRCUIT DISCOVERED! 🚨");
                let _ = writeln!(
                    file,
                    "=> Time Elapsed   : {:.3} seconds",
                    self.start_time.elapsed().as_secs_f32()
                );
                let _ = writeln!(
                    file,
                    "=> Assembly Index : {} Active Gates",
                    active_gate_count
                );

                // Write the full netlist textually
                for (i, gate) in candidate.gates.iter().enumerate() {
                    let gate_wire_idx = self.num_inputs + i;
                    if active_wires[gate_wire_idx] {
                        let l_str = if (gate.left as usize) < self.num_inputs {
                            format!("INP_{}", gate.left)
                        } else {
                            format!("W_{}", gate.left)
                        };
                        let r_str = if (gate.right as usize) < self.num_inputs {
                            format!("INP_{}", gate.right)
                        } else {
                            format!("W_{}", gate.right)
                        };
                        if gate.op == GateOp::Not {
                            let _ = writeln!(file, "  W_{:<2} = NOT {}", gate_wire_idx, l_str);
                        } else {
                            let _ = writeln!(
                                file,
                                "  W_{:<2} = {} {:?} {}",
                                gate_wire_idx, l_str, gate.op, r_str
                            );
                        }
                    }
                }
                for j in 0..self.num_outputs {
                    let out_wire_idx = candidate.output_map[j] as usize;
                    let out_str = if out_wire_idx < self.num_inputs {
                        format!("INP_{}", out_wire_idx)
                    } else {
                        format!("W_{}", out_wire_idx)
                    };
                    let _ = writeln!(file, "  OUT_{} = {}", j, out_str);
                }
                let _ = file.sync_all(); // Hard flush to disk to prevent data loss on crash
            }

            return true;
        }
        false
    }

    fn distill_theory(
        &self,
        old_candidate: &AsicCircuit,
        new_candidate: &AsicCircuit,
        fitness_jump: u32,
    ) {
        let diff_text = format!(
            "FITNESS JUMP +{}. Old gates: {}, New gates: {}",
            fitness_jump,
            old_candidate.gates.len(),
            new_candidate.gates.len()
        );
        let motif = new_candidate.clone();

        let motifs_db = self.cached_motifs.clone();

        tokio::task::spawn_blocking(move || {
            let _tracker = no_std_tool::debug::ScopedResource::new();
            // Hardcoded token IDs for Gemma 2B: Low=5687, Medium=10555, High=3665, Critical=9906
            let candidates = [5687, 10555, 3665, 9906];
            let best_token_res = crate::assembly::router::get_fallback_engine()
                .classify_logits(&diff_text, &candidates);

            if let Ok(best_token) = best_token_res {
                // If High (3665) or Critical (9906)
                if best_token == 3665 || best_token == 9906 {
                    println!(
                        "[LLM-Guided Symbolic Search] Distilled Theory! Caching macro into DualCacheFF. Token = {}",
                        best_token
                    );

                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    motif.gates.hash(&mut hasher);
                    let motif_id = hasher.finish();

                    // Assuming 8 inputs for standard S-Box/Circuit macros
                    // Register thread for TLS handle
                    let handle = motifs_db.register_thread();
                    motifs_db.insert(motif_id, Arc::new((8, motif)), &handle);
                } else {
                    println!(
                        "[LLM-Guided Symbolic Search] Rejected Theory (Low/Medium Reward). Token = {}",
                        best_token
                    );
                }
            }
        });
    }

    fn crossover(
        &self,
        parent_a: &AsicCircuit,
        parent_b: &AsicCircuit,
        mut seed: usize,
    ) -> [AsicCircuit; 4] {
        let mut child1 = parent_a.clone();
        let mut child2 = parent_b.clone();
        let mut child3 = parent_a.clone();
        let mut child4 = parent_b.clone();

        let min_len = child1.gates.len().min(child2.gates.len());

        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let split = seed % min_len.max(1);

        // 1-point crossover (Child 1 & 2)
        let (left_a, right_a) = parent_a.gates.split_at(split);
        let (left_b, right_b) = parent_b.gates.split_at(split);

        child1.gates = left_a
            .iter()
            .cloned()
            .chain(right_b.iter().cloned())
            .collect();
        child2.gates = left_b
            .iter()
            .cloned()
            .chain(right_a.iter().cloned())
            .collect();

        // Uniform crossover (Child 3 & 4)
        child3.gates.clear();
        child4.gates.clear();
        for i in 0..min_len {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            if seed.is_multiple_of(2) {
                child3.gates.push(parent_b.gates[i].clone());
                child4.gates.push(parent_a.gates[i].clone());
            } else {
                child3.gates.push(parent_a.gates[i].clone());
                child4.gates.push(parent_b.gates[i].clone());
            }
        }
        // Append the remaining gates from the longer parent for child3 and child4
        if parent_a.gates.len() > min_len {
            child3
                .gates
                .extend(parent_a.gates[min_len..].iter().cloned());
            child4
                .gates
                .extend(parent_a.gates[min_len..].iter().cloned());
        } else if parent_b.gates.len() > min_len {
            child3
                .gates
                .extend(parent_b.gates[min_len..].iter().cloned());
            child4
                .gates
                .extend(parent_b.gates[min_len..].iter().cloned());
        }

        // Output Map Uniform Crossover
        for i in 0..self.num_outputs {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            if seed.is_multiple_of(2) {
                child1.output_map[i] = parent_b.output_map[i];
                child2.output_map[i] = parent_a.output_map[i];
                child3.output_map[i] = parent_b.output_map[i];
                child4.output_map[i] = parent_a.output_map[i];
            }
        }

        // Clamp indices to prevent out-of-bounds wires
        self.clamp_circuit(&mut child1);
        self.clamp_circuit(&mut child2);
        self.clamp_circuit(&mut child3);
        self.clamp_circuit(&mut child4);

        [child1, child2, child3, child4]
    }
}
