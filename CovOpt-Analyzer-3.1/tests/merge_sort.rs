use covopt_macro::{covopt_param, covopt_test};
use std::hint::black_box;

#[inline(never)]
pub fn compute_merge_sort(n: usize) {
    let mut vec: Vec<usize> = (0..n).rev().collect();
    let sort_passes = covopt_param!("MERGE_SORT_PASSES", 1);
    for _ in black_box(0..sort_passes) {
        vec.sort();
        black_box(&vec);
    }
}

#[cfg(test)]
#[covopt_test(
    target_fn = "compute_merge_sort",
    expected = "O(N log N)",
    n_values = "100,500,1000"
)]
fn merge_sort(n: usize) {
    compute_merge_sort(n);
}
