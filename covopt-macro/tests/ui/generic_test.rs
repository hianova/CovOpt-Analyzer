use covopt_macro::covopt_test;

#[covopt_test]
fn generic_test<T>(n: usize) {
    let _ = n;
    let _ = core::marker::PhantomData::<T>;
}

fn main() {}
