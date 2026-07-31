use covopt_macro::covopt_param;

fn main() {
    let _ = covopt_param!("field", 1, unknown = "value");
}
