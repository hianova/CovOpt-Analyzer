use crate::science::assembly_funnel::FunnelObserver;
use std::sync::Mutex;

static THEORY_ARCHIVE: std::sync::LazyLock<Mutex<Vec<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

fn get_theory_archive() -> &'static Mutex<Vec<String>> {
    &THEORY_ARCHIVE
}

pub struct AutoResearchConfig {
    pub mode: String, // e.g. "CPU", "GPU", "Dual"
}

pub struct AutoResearcher {
    pub config: AutoResearchConfig,
    pub log_gen: bool,
    pub prefix: String,
}

impl AutoResearcher {
    pub fn new(config: AutoResearchConfig) -> Self {
        Self {
            config,
            log_gen: true,
            prefix: "[AutoResearcher]".to_string(),
        }
    }

    pub fn with_generation_log(mut self, log_gen: bool) -> Self {
        self.log_gen = log_gen;
        self
    }

    pub fn get_active_rules() -> Vec<String> {
        get_theory_archive().lock().unwrap().clone()
    }
}

impl FunnelObserver for AutoResearcher {
    fn on_step(&mut self, msg: &str) {
        println!("{} {}", self.prefix, msg);
    }

    fn on_success(&mut self, msg: &str) {
        println!("{} \x1b[1;32mSuccess:\x1b[0m {}", self.prefix, msg);
    }

    fn on_warning(&mut self, msg: &str) {
        println!("{} \x1b[1;33mWarning:\x1b[0m {}", self.prefix, msg);
    }

    fn on_error(&mut self, msg: &str) {
        println!("{} \x1b[1;31mError:\x1b[0m {}", self.prefix, msg);
    }

    fn confirm(&mut self, prompt: &str) -> bool {
        use std::io::Write;
        print!("{} \x1b[1m{} [y/N]\x1b[0m ", self.prefix, prompt);
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let t = input.trim().to_lowercase();
            t == "y" || t == "yes"
        } else {
            false
        }
    }

    fn on_generation_complete(
        &mut self,
        generation: u64,
        global_iters: u64,
        _best_fitness: (u32, u32),
        total_found: usize,
    ) {
        if self.log_gen {
            self.on_step(&format!(
                "Gen {} Complete. Iters: {}, Found: {}",
                generation, global_iters, total_found
            ));
        }
    }

    fn on_archive_success(&mut self, generation: u64, _global_iters: u64, fitness: (u32, u32)) {
        self.on_success(&format!(
            "Archive Success! Gen: {}, Fitness: {:?}",
            generation, fitness
        ));
    }
}
