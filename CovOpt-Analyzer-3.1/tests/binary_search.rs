use covopt_macro::{covopt_evidence, covopt_param, covopt_target, covopt_test};
use std::hint::black_box;

#[inline(never)]
#[covopt_target(id = "binary_search", complexity = "O(log N)")]
pub fn compute_binary_search(n: usize) {
    let vec: Vec<usize> = (0..n).collect();
    let iters = covopt_param!("BINARY_SEARCH_ITERS", 100);
    let target = covopt_param!("BINARY_SEARCH_TARGET", 42) % n.max(1);

    for _ in black_box(0..iters) {
        let res = vec.binary_search(&black_box(target));
        let _ = black_box(res);
    }
    black_box(vec);
}

#[cfg(test)]
#[covopt_evidence(target = "binary_search", n = [1000, 5000, 10000], seeds = "adaptive")]
#[covopt_test(
    target_fn = "compute_binary_search",
    expected = "O(log N)",
    n_values = "1000,5000,10000"
)]
fn binary_search(n: usize) {
    compute_binary_search(n);
}
