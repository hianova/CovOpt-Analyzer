use covopt_macro::covopt_param;

#[inline(never)]
pub fn default_value() -> usize {
    covopt_param!("codegen::default", 64usize)
}

#[test]
fn default_value_is_the_literal() {
    assert_eq!(default_value(), 64);
}
