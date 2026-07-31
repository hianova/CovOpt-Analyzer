#![no_std]

extern crate std;

use covopt_macro::covopt_param;

const NEGATIVE: i32 = covopt_param!("const::negative", -3i32, evaluation = "compile_time");
const FLOAT: f64 = covopt_param!("const::float", 1.5f64, evaluation = "compile_time");
const ARRAY: [u8; covopt_param!("const::array_len", 4usize, evaluation = "compile_time")] = [0; 4];

#[test]
fn compile_time_and_no_std_expansions_are_plain_values() {
    assert_eq!(NEGATIVE, -3);
    assert_eq!(FLOAT, 1.5);
    assert_eq!(ARRAY.len(), 4);
}
