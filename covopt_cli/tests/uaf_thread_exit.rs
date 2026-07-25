use covopt_macro::covopt_param;
use std::thread;
use std::time::Duration;

#[test]
#[ignore = "Intentionally crashes the process to test sanitizer"]
fn test_uaf_on_thread_exit() {
    let mut data = Box::new(covopt_param!("M_12_28", 42));
    let ptr = data.as_mut() as *mut i32;
    let ptr_addr = ptr as usize;

    let handle = thread::spawn(move || {
        let ptr = ptr_addr as *mut i32;
        unsafe {
            drop(Box::from_raw(ptr));
            thread::sleep(Duration::from_millis(covopt_param!("M_24_48", 50)));
            println!("Read after free: {}", *ptr);
        }
    });

    handle.join().unwrap();
}
