#[test]
fn no_macro_test() {
    let n: usize = std::env::var("COVOPT_N").unwrap_or_else(|_| "100".to_string()).parse().unwrap();
    let mut sum = 0;
    for i in 0..n {
        sum += i;
        std::hint::black_box(sum);
    }
}
