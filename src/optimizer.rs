use crate::mca::McaRunner;
use covopt_macro::covopt_param;
use std::collections::HashSet;

#[derive(Clone)]
struct InstMeta {
    id: usize,
    writes: Vec<String>,
    reads: Vec<String>,
    is_mem: bool,
    is_branch: bool,
}

pub struct DiscreteDiffusionEngine {
    pub diffusion_steps: usize,
}

impl Default for DiscreteDiffusionEngine {
    fn default() -> Self {
        Self {
            diffusion_steps: covopt_param!("M_20_29", 10),
        }
    }
}

impl DiscreteDiffusionEngine {
    pub fn new(diffusion_steps: usize) -> Self {
        Self { diffusion_steps }
    }

    fn parse_inst(id: usize, line: &str) -> InstMeta {
        let mut writes = Vec::new();
        let mut reads = Vec::new();
        let is_mem = line.contains('[') || line.contains(']');
        let is_branch = line.contains(" b ")
            || line.contains("\tb")
            || line.contains("cbz")
            || line.contains("cbnz")
            || line.contains("ret");
        let sets_flags = line.contains("cmp")
            || line.contains("adds")
            || line.contains("subs")
            || line.contains("ands");
        let reads_flags = line.contains("b.") || line.contains("csel");

        if sets_flags {
            writes.push("NZCV".to_string());
        }
        if reads_flags {
            reads.push("NZCV".to_string());
        }

        // Basic tokenizer for AArch64 operands
        let parts: Vec<&str> = line
            .split(&[' ', '\t', ',', '[', ']', '!'][..])
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() > 1 {
            let op = parts[0];
            // Usually the first operand is the destination register, except for store instructions
            let is_store = op.starts_with("st");

            for (i, part) in parts.iter().skip(1).enumerate() {
                if (part.starts_with('x')
                    || part.starts_with('w')
                    || part.starts_with('v')
                    || part.starts_with('q'))
                    && part.len() <= covopt_param!("M_67_37", 3)
                    && part[1..].parse::<u32>().is_ok()
                {
                    if i == 0 && !is_store {
                        writes.push(part.to_string());
                    } else {
                        reads.push(part.to_string());
                    }
                }
            }
        }

        InstMeta {
            id,
            writes,
            reads,
            is_mem,
            is_branch,
        }
    }

    fn build_dependency_edges(base_asm: &[String]) -> HashSet<(usize, usize)> {
        let mut edges = HashSet::new();
        let mut metas = Vec::new();
        for (i, line) in base_asm.iter().enumerate() {
            metas.push(Self::parse_inst(i, line));
        }

        // RAW, WAR, WAW dependencies
        for (i, a) in metas.iter().enumerate() {
            for b in metas.iter().skip(i + 1) {
                let mut dependent = false;

                // Memory ordering
                if a.is_mem && b.is_mem {
                    dependent = true;
                }
                // Branch ordering
                if a.is_branch || b.is_branch {
                    dependent = true;
                }

                // Register dependencies
                for w in &a.writes {
                    if b.reads.contains(w) || b.writes.contains(w) {
                        dependent = true;
                    }
                }
                for r in &a.reads {
                    if b.writes.contains(r) {
                        dependent = true;
                    }
                }

                if dependent {
                    edges.insert((a.id, b.id));
                }
            }
        }
        edges
    }

    fn is_valid_permutation(
        candidate: &[(usize, String)],
        edges: &HashSet<(usize, usize)>,
    ) -> bool {
        for i in 0..candidate.len() {
            for j in (i + 1)..candidate.len() {
                let id_a = candidate[i].0;
                let id_b = candidate[j].0;
                // If id_b must come before id_a according to edges, but id_a is before id_b in candidate, invalid!
                if edges.contains(&(id_b, id_a)) {
                    return false;
                }
            }
        }
        true
    }

    pub fn optimize_asm(
        &self,
        base_asm: Vec<String>,
        num_candidates: usize,
        mca_cpu: Option<String>,
    ) -> Vec<String> {
        if num_candidates == 0 {
            return base_asm;
        }

        let edges = Self::build_dependency_edges(&base_asm);

        // Base candidate includes IDs
        let base_candidate: Vec<(usize, String)> = base_asm.into_iter().enumerate().collect();

        let mut seeds = Vec::with_capacity(num_candidates);
        let mut base_seed: usize = covopt_param!("M_162_28", 12345);
        for _ in 0..num_candidates {
            base_seed = base_seed
                .wrapping_mul(covopt_param!("M_164_47", 1664525))
                .wrapping_add(covopt_param!("M_164_69", 1013904223));
            seeds.push(base_seed);
        }

        // Initialize canvas
        let mut canvas: Vec<Vec<(usize, String)>> = vec![base_candidate.clone(); num_candidates];
        let mca_runner = McaRunner::new(mca_cpu.clone());

        for step in 0..self.diffusion_steps {
            let noise_level = 1.0 - (step as f64 / self.diffusion_steps as f64);

            for (i, candidate) in canvas.iter_mut().enumerate() {
                let step_seed = seeds[i].wrapping_add(step * covopt_param!("M_176_61", 1234567));
                Self::mutate_asm(candidate, noise_level, step_seed);
            }

            // Evaluate and keep the best
            let mut scored_candidates: Vec<(f64, Vec<(usize, String)>)> = canvas
                .iter()
                .map(|candidate| {
                    if !Self::is_valid_permutation(candidate, &edges) {
                        return (f64::MAX, candidate.clone());
                    }

                    let asm_text = candidate
                        .iter()
                        .map(|(_, s)| s.clone())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let score = match mca_runner.run(&asm_text) {
                        Ok(report) => {
                            report.block_rthroughput
                                - (report.ipc * covopt_param!("M_194_79", 0.001))
                        } // Combine RThroughput and IPC
                        Err(_) => f64::MAX,
                    };
                    (score, candidate.clone())
                })
                .collect();

            scored_candidates
                .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

            let best = scored_candidates[0].1.clone();
            let best_score = scored_candidates[0].0;

            for c in canvas.iter_mut() {
                *c = best.clone();
            }

            if best_score != f64::MAX {
                println!(
                    "  [Optimizer] Step {}/{} - Best Objective Score: {:.4}",
                    step + 1,
                    self.diffusion_steps,
                    best_score
                );
            } else {
                println!(
                    "  [Optimizer] Step {}/{} - No valid permutations found yet",
                    step + 1,
                    self.diffusion_steps
                );
            }
        }

        canvas[0].iter().map(|(_, s)| s.clone()).collect()
    }

    fn mutate_asm(candidate: &mut [(usize, String)], noise_level: f64, mut seed: usize) {
        let num_mutations =
            (candidate.len() as f64 * noise_level * covopt_param!("M_231_68", 0.1)).ceil() as usize;

        let rand = |s: &mut usize, max: usize| -> usize {
            *s = s
                .wrapping_mul(covopt_param!("M_234_32", 1664525))
                .wrapping_add(covopt_param!("M_234_54", 1013904223));
            *s % max
        };

        for _ in 0..num_mutations {
            if candidate.len() < 2 {
                break;
            }
            let i = rand(&mut seed, candidate.len());
            let j = rand(&mut seed, candidate.len());

            let is_inst = |s: &str| {
                let trimmed = s.trim();
                !trimmed.is_empty() && !trimmed.starts_with('.') && !trimmed.ends_with(':')
            };

            if is_inst(&candidate[i].1) && is_inst(&candidate[j].1) {
                candidate.swap(i, j);
            }
        }
    }
}
