use crate::science::chaos_state::ChaosEngram;
use std::fs;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug)]
pub enum DomainContext {
    Music { tension: f64, density: f64 },
    Architecture { height: f64, stress: f64 },
    Mechanics { degrees_of_freedom: f64 },
}

pub struct ExperienceOracle {
    pub state: ChaosEngram,
    pub stagnation_counter: usize,
    pub genome_dimension: usize,
}

static ORACLE_INSTANCE: OnceLock<Mutex<ExperienceOracle>> = OnceLock::new();

pub fn get_oracle() -> &'static Mutex<ExperienceOracle> {
    ORACLE_INSTANCE.get_or_init(|| {
        let state = match fs::read_to_string("searu_chaos.engram") {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => ChaosEngram::default(),
        };

        Mutex::new(ExperienceOracle {
            state,
            stagnation_counter: 0,
            genome_dimension: 10,
        })
    })
}

impl ExperienceOracle {
    /// Queries the internal chaos state to predict the starting prior distribution
    pub fn predict_prior(&mut self, _context: DomainContext) -> (f64, f64) {
        // Use the saved energy level from ChaosState to influence the starting temperature
        let prior_temp = (self.state.energy_level * 100.0).clamp(10.0, 100.0);
        let bounds_scale = 1.0 + (self.state.fitness * 0.5);

        (prior_temp, bounds_scale)
    }

    /// Feedback loop: Called after annealing finishes a run.
    /// Now, it directly saves the ChaosState rather than tweaking neural net weights.
    pub fn learn_chaos(&mut self, fitness: f64, is_epiphany: bool, final_temp: f64, seed: u64) {
        if is_epiphany {
            self.stagnation_counter = 0;
            self.state.fitness = fitness;
            self.state.seed = seed;
            self.state.energy_level = final_temp;

            if let Ok(json) = serde_json::to_string_pretty(&self.state) {
                let _ = fs::write("searu_chaos.engram", json);
            }
        } else {
            self.stagnation_counter += 1;
            if self.stagnation_counter >= 10 {
                self.trigger_paradigm_shift();
            }
        }
    }

    pub fn trigger_paradigm_shift(&mut self) {
        self.stagnation_counter = 0;
        self.genome_dimension += 1;
        
        // Reset the seed to induce a massive chaos shift
        self.state.seed ^= 0xCAFEBABE;
        self.state.energy_level = 1.0;

        println!("\n=======================================================");
        println!("📈 [Dynamic Scaling] Optimization stagnation detected.");
        println!("📈 Expanding search space dimensions to {}D", self.genome_dimension);
        println!("📈 Paradigm Shift Triggered: Chaos Seed Re-randomized!");
        println!("=======================================================\n");
    }
}
