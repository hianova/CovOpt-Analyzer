use covopt_macro::{covopt_param, covopt_test};
use std::hint::black_box;

#[inline(never)]
pub fn compute_linear_scan(n: usize) -> usize {
    let vec: Vec<usize> = (0..n).collect();
    let mut sum: usize = covopt_param!("LINEAR_SCAN_INIT_SUM", 0);
    for val in black_box(&vec) {
        sum = sum.wrapping_add(black_box(*val));
    }
    black_box(sum)
}

#[cfg(test)]
#[covopt_test(
    target_fn = "compute_linear_scan",
    expected = "O(N)",
    n_values = "1000,5000,10000"
)]
fn linear_scan(n: usize) {
    compute_linear_scan(n);
}
