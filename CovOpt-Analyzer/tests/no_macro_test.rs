use covopt_macro::covopt_param;
use std::hint::black_box;

#[test]
fn no_macro_test() {
    let default_n_str = covopt_param!("NO_MACRO_DEFAULT_N", "100".to_string());
    let n: usize = std::env::var("COVOPT_N")
        .unwrap_or_else(|_| default_n_str.to_string())
        .parse()
        .unwrap();
    let mut sum = 0;
    for i in black_box(0..n) {
        sum += i;
        black_box(sum);
    }
}
