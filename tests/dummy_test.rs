use covopt_macro::covopt_test;

#[covopt_test(expected = O(N), n_values = [1000, 5000, 10000])]
fn dummy_algorithm(n: usize) {
    let mut sum = 0;
    for i in 0..n {
        sum += i;
    }
    std::hint::black_box(sum);
}
