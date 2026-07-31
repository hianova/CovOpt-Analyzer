use covopt_macro::{covopt_hoist, covopt_qsbr_registry};

#[covopt_hoist(capacity = 2, partition = "test_pool")]
struct Hoisted {
    value: usize,
}

struct QNode {
    #[allow(dead_code)]
    value: usize,
}

impl QNode {
    fn new() -> Self {
        Self { value: 0 }
    }
}

unsafe fn register(_node: *mut QNode) {}
unsafe fn unregister(_node: *mut QNode) {}

covopt_qsbr_registry!(
    pub struct Registry;
    node_type = QNode;
    register = register;
    unregister = unregister;
);

#[test]
fn unsafe_macros_compile_with_explicit_lifetime_hooks() {
    let mut token = TestPoolToken;
    let index = unsafe { Hoisted::insert(Hoisted { value: 7 }, &mut token) }.unwrap();
    assert_eq!(unsafe { Hoisted::get(index, &token) }.unwrap().value, 7);
    assert_eq!(
        unsafe { Hoisted::remove(index, &mut token) }.unwrap().value,
        7
    );
    assert!(!Registry::pin().is_null());
}
