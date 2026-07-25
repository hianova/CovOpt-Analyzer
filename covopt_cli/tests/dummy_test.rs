use covopt_macro::{covopt_param, covopt_test};
use std::hint::black_box;

#[inline(never)]
pub fn compute_dummy_algorithm(n: usize) -> usize {
    let mut sum: usize = covopt_param!("DUMMY_INIT_SUM", 0);
    for i in black_box(0..n) {
        sum = sum.wrapping_add(i);
        black_box(sum);
    }
    black_box(sum)
}

#[cfg(test)]
#[covopt_test(target_fn = "compute_dummy_algorithm", expected = "O(N)", n_values = "1000,5000,10000")]
fn dummy_algorithm(n: usize) {
    compute_dummy_algorithm(n);
}
