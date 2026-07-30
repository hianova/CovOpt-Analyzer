use std::fs;
use std::path::Path;
use syn::visit_mut::VisitMut;
use syn::{Expr, Stmt, ItemFn};

pub struct ConcurrencyFuzzerMutator {
    pub delay_count: usize,
}

impl VisitMut for ConcurrencyFuzzerMutator {
    fn visit_expr_mut(&mut self, node: &mut Expr) {
        let mut should_inject = false;

        match node {
            Expr::MethodCall(mcall) => {
                let method_name = mcall.method.to_string();
                if is_critical_method(&method_name) {
                    should_inject = true;
                }
            }
            Expr::Call(call) => {
                if let Expr::Path(expr_path) = &*call.func
                    && let Some(seg) = expr_path.path.segments.last()
                        && is_critical_function(&seg.ident.to_string()) {
                            should_inject = true;
                        }
            }
            Expr::Await(_) => {
                should_inject = true; // Inject before .await
            }
            _ => {}
        }

        // Visit inner nodes first to transform them from bottom-up
        syn::visit_mut::visit_expr_mut(self, node);

        if should_inject {
            let delay_idx = self.delay_count;
            self.delay_count += 1;
            
            // We want to transform `expr` into `{ covopt_fuzzer::spin_delay(idx); expr }`
            let original_expr = node.clone();
            
            let block_expr = syn::parse_quote!({
                covopt_fuzzer::spin_delay(#delay_idx);
                #original_expr
            });
            *node = Expr::Block(block_expr);
        }
    }
    
    fn visit_expr_unsafe_mut(&mut self, node: &mut syn::ExprUnsafe) {
        syn::visit_mut::visit_expr_unsafe_mut(self, node);
        
        let delay_idx = self.delay_count;
        self.delay_count += 1;
        
        // Inject at the beginning of the unsafe block
        let delay_stmt: Stmt = syn::parse_quote!(covopt_fuzzer::spin_delay(#delay_idx););
        node.block.stmts.insert(0, delay_stmt);
    }
    fn visit_item_fn_mut(&mut self, node: &mut ItemFn) {
        syn::visit_mut::visit_item_fn_mut(self, node);
        
        if node.attrs.iter().any(|attr| attr.path().is_ident("test")) {
            let original_block = &node.block;
            let new_block = syn::parse_quote!({
                covopt_fuzzer::run_fuzz_loop(|_covopt_iter| {
                    #original_block
                });
            });
            *node.block = new_block;
        }
    }
}

fn is_critical_method(name: &str) -> bool {
    matches!(
        name,
        "load" | "store" | "swap" | "compare_exchange" | "compare_exchange_weak" |
        "fetch_add" | "fetch_sub" | "fetch_and" | "fetch_nand" | "fetch_or" | "fetch_xor" | "fetch_max" | "fetch_min" | "fetch_update" |
        "lock" | "read" | "write" | "wait" | "notify_one" | "notify_all" | "call_once" |
        "send" | "recv" | "try_send" | "try_recv" |
        "join" | "borrow" | "borrow_mut" | "replace" | "take"
    )
}

fn is_critical_function(name: &str) -> bool {
    matches!(
        name,
        "spawn" | "yield_now" | "park" | "unpark"
    )
}

pub fn instrument_test_file(path: &Path, out_path: &Path) -> Result<usize, String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut ast = syn::parse_file(&content).map_err(|e| e.to_string())?;

    let mut mutator = ConcurrencyFuzzerMutator { delay_count: 0 };
    mutator.visit_file_mut(&mut ast);

    // Inject Harness
    let harness_code = quote::quote! {
        pub mod covopt_fuzzer {
            use std::sync::atomic::{AtomicU32, Ordering};
            use std::time::Duration;
            use std::thread;

            const INIT: AtomicU32 = AtomicU32::new(0);
            pub static FUZZ_DELAYS: [AtomicU32; 10000] = [INIT; 10000];

            #[inline(always)]
            pub fn spin_delay(idx: usize) {
                if idx < FUZZ_DELAYS.len() {
                    let delay = FUZZ_DELAYS[idx].load(Ordering::Relaxed);
                    if delay > 0 {
                        if delay > 10000 {
                            thread::sleep(Duration::from_nanos(delay as u64));
                        } else {
                            for _ in 0..delay {
                                std::hint::spin_loop();
                            }
                        }
                    }
                }
            }

            pub fn run_fuzz_loop<F: Fn(usize) + std::panic::RefUnwindSafe>(f: F) {
                println!("🚀 Starting In-Process Adversarial Concurrency Fuzzer...");
                // Simple random fuzzer loop
                let iterations = 10000; // testing with 10k for now
                let mut rng_seed: u64 = 0x12345678; // LCG state
                
                for i in 0..iterations {
                    // Fast pseudo-random generation for delays
                    // Sparse mutation: only inject delay in 5% of locations per iteration
                    for j in 0..500 { // Max 500 delays modeled
                        rng_seed = rng_seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        if (rng_seed >> 32) % 20 == 0 {
                            // Inject a delay between 0 and 100000 ns (0.1ms)
                            FUZZ_DELAYS[j].store(((rng_seed >> 16) % 100000) as u32, Ordering::Relaxed);
                        } else {
                            FUZZ_DELAYS[j].store(0, Ordering::Relaxed);
                        }
                    }

                    let result = std::panic::catch_unwind(|| {
                        f(i)
                    });
                    
                    if result.is_err() {
                        println!("💥 [CRASH DETECTED] Fuzzer found a concurrency bug at iteration {}!", i);
                        print!("Failing Delay Matrix (first 20 injected points): [");
                        for k in 0..20 {
                            print!("{}, ", FUZZ_DELAYS[k].load(Ordering::Relaxed));
                        }
                        println!("...]");
                        std::process::exit(1);
                    }
                }
                println!("✅ Fuzzing Complete. {} generations tested. No bugs found.", iterations);
            }
        }
    };
    
    let modified_code = quote::quote! {
        #harness_code
        #ast
    };

    fs::write(out_path, modified_code.to_string()).map_err(|e| e.to_string())?;

    Ok(mutator.delay_count)
}

pub fn run_fuzzer(args: &CovOpt_Analyzer::config::FuzzArgs) -> Result<(), String> {
    let target_path = Path::new(&args.target);
    if !target_path.exists() {
        return Err(format!("Target file not found: {}", args.target));
    }
    
    // Determine output path (e.g., .covopt/fuzz_target.rs or just alongside)
    let file_name = target_path.file_name().unwrap().to_str().unwrap();
    let fuzz_file_name = format!("covopt_fuzzed_{}", file_name);
    let fuzz_target_path = target_path.with_file_name(&fuzz_file_name);
    
    println!("🔍 Analyzing and instrumenting {}", args.target);
    let points = instrument_test_file(target_path, &fuzz_target_path)?;
    println!("💉 Injected {} adversarial delay points into AST", points);
    println!("📝 Fuzzing harness saved to {}", fuzz_target_path.display());
    
    println!("⚡ Compiling and running In-Process Fuzzing Engine...");
    
    // Now we must run cargo test on the fuzzed file
    // To do this simply, we run cargo test --test <name_without_rs>
    // However, if target is tests/xxx.rs, we need to run it.
    let test_name = fuzz_file_name.replace(".rs", "");
    
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["test", "--test", &test_name, "--", "--nocapture"]);
    
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    
    if !status.success() {
        println!("💥 [BUG DETECTED] Fuzzer found a concurrency bug!");
    } else {
        println!("✅ Fuzzer completed without finding any bugs.");
    }
    
    // Clean up
    let _ = fs::remove_file(&fuzz_target_path);
    
    Ok(())
}

