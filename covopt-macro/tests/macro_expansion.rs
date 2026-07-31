use covopt_macro::{
    covopt_atomic, covopt_bench, covopt_evidence, covopt_param, covopt_target, covopt_test,
};

const DEFAULT_QUEUE: usize = covopt_param!(
    "queue.spin",
    64,
    1..=4096,
    class = "capacity",
    range = 1..=4096,
    scale = "pow2",
    unit = "iterations",
    risk = ["latency", "liveness"]
);

#[covopt_target(id = "fixture::work", complexity = "O(N)", criticality = "normal")]
fn contract_target() -> usize {
    DEFAULT_QUEUE
}

#[covopt_evidence(target = "fixture::work", n = [1, 8, 64], seeds = "7,11", threads = [1, 2])]
#[allow(dead_code)]
fn contract_evidence() {}

#[covopt_atomic(target = "fixture::work", ordering = "acq-rel", liveness = "bounded")]
#[allow(dead_code)]
fn atomic_contract() {}

#[covopt_bench]
fn benchmark_marker() -> usize {
    covopt_param!("bench::limit", 8, class = "threshold")
}

#[covopt_test]
fn adapter_injects_trial_axes(n: usize, seed: u64, threads: usize) {
    assert!(n > 0);
    assert_eq!(seed, 0);
    assert_eq!(threads, 1);
}

#[covopt_test]
fn adapter_propagates_result(size: usize) -> Result<(), &'static str> {
    if size > 0 {
        Ok(())
    } else {
        Err("injected size must be positive")
    }
}

#[test]
fn default_expansion_is_a_plain_expression() {
    assert_eq!(DEFAULT_QUEUE, 64);
    assert_eq!(contract_target(), 64);
    assert_eq!(benchmark_marker(), 8);
}
