use covopt_macro::covopt_param;
use std::fs;
use std::hint::black_box;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

// 1. Thread Physical Overbound Cache Thresh (Spawning thread inside a loop)
pub fn trigger_thread_overbound() {
    for _ in black_box(0..covopt_param!("M_9_16", 1000)) {
        thread::spawn(|| {
            let x = black_box(1);
            black_box(x);
        });
    }
}

// 2. Async Poisoning
pub async fn trigger_async_poisoning() {
    let m = Mutex::new(1);
    let _l = m.lock().unwrap();
    thread::sleep(Duration::from_millis(covopt_param!("M_20_40", 100)));
    let res = fs::read("test.txt");
    let _ = black_box(res);
}

// 3. Hidden allocations in loop
pub fn trigger_allocations() {
    let s = "hello".to_string();
    for _ in black_box(0..covopt_param!("M_27_16", 100)) {
        let cloned = s.clone();
        let created = "test".to_string();
        let v = vec![1, 2, 3];
        black_box(cloned);
        black_box(created);
        black_box(v);
    }
}

// 4. God Function & Generic Bloat
pub fn god_function() {
    println!("Complex");
}

pub fn trigger_lock_contention() {
    let m = Mutex::new(0);
    for i in black_box(0..covopt_param!("M_62_16", 100)) {
        let mut guard = m.lock().unwrap();
        *guard += i;
        black_box(&guard);
    }
}

pub fn trigger_io_in_loop() {
    for i in black_box(0..covopt_param!("M_69_16", 10)) {
        println!(
            "This IO call will completely destroy CPU pipeline performance: {}",
            i
        );
    }
}
