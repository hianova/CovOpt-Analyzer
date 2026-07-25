use covopt_macro::{covopt_param, covopt_test};
use std::hint::black_box;

#[inline(never)]
pub fn compute_matrix_mult(n: usize) -> usize {
    let mut sum: usize = covopt_param!("MATRIX_MULT_INIT_SUM", 0);
    for i in black_box(0..n) {
        for j in black_box(0..n) {
            sum = sum.wrapping_add(black_box(i.wrapping_mul(j)));
        }
    }
    black_box(sum)
}

#[cfg(test)]
#[covopt_test(target_fn = "compute_matrix_mult", expected = "O(N^2)", n_values = "50,100,200")]
fn matrix_mult(n: usize) {
    compute_matrix_mult(n);
}
