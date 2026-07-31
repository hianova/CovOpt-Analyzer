use covopt_macro::covopt_param;

fn main() {
    let _ = covopt_param!("duplicate", 1, class = "threshold", class = "capacity");
}
