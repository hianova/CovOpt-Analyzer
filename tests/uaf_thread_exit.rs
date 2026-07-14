use std::thread;
use std::time::Duration;

#[test]
#[ignore = "Intentionally crashes the process to test sanitizer"]
fn test_uaf_on_thread_exit() {
    // This test deliberately creates a Use-After-Free (UAF) bug
    // to verify that CovOpt-Analyzer's sanitizer integration (AddressSanitizer)
    // can catch it during thread exit.

    // Allocate a Box on the heap
    let mut data = Box::new(42);
    // Get a raw pointer to it
    let ptr = data.as_mut() as *mut i32;
    let ptr_addr = ptr as usize; // Cast to usize to allow moving across thread

    let handle = thread::spawn(move || {
        let ptr = ptr_addr as *mut i32;
        unsafe {
            // 1. Manually drop the box, freeing the memory.
            drop(Box::from_raw(ptr));

            // 2. Wait a little bit to ensure it's freed
            thread::sleep(Duration::from_millis(50));

            // 3. Read from the freed memory just before the thread exits
            // This is a Use-After-Free! AddressSanitizer should crash here.
            println!("Read after free: {}", *ptr);
        }
    });

    handle.join().unwrap();
}
