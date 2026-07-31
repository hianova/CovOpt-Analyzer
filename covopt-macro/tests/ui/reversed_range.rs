use covopt_macro::covopt_param;

fn main() {
    let _ = covopt_param!("range", 1, range = 4..=1);
}
