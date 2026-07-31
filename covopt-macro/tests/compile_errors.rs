#[test]
fn invalid_metadata_is_reported_as_compile_error() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
}
