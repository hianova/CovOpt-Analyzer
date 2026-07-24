#![allow(dead_code)]
use covopt_macro::covopt_param;
#[allow(unused_imports)]
use std::thread;
use std::time::Duration;
use std::fs;

// 1. Thread Physical Overbound Cache Thresh (Spawning thread inside a loop)
fn trigger_thread_overbound() {
    for _ in 0..covopt_param!("M_9_16", 1000) {
        thread::spawn(|| {
            let _x = 1;
        });
    }
}

// 2. Async Poisoning
async fn trigger_async_poisoning() {
    let m = Mutex::new(1);
    let _l = m.lock().unwrap();
    thread::sleep(Duration::from_millis(covopt_param!("M_20_40", 100)));
    let _ = fs::read("test.txt");
}

// 3. Hidden allocations in loop
fn trigger_allocations() {
    let s = "hello".to_string();
    for _ in 0..covopt_param!("M_27_16", 100) {
        let _ = s.clone();
        let _ = format!("test");
        let _ = vec![1, 2, 3];
    }
}

// 4. God Function & Generic Bloat
fn god_function<A, B, C, D>() {
    if true {
        if true {
            if true {
                if true {
                    if true {
                        if true {
                            if true {
                                if true {
                                    if true {
                                        if true {
                                            println!("Complex");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

use std::sync::Mutex;
fn trigger_lock_contention() {
    let m = Mutex::new(0);
    for i in 0..covopt_param!("M_62_16", 100) {
        let mut guard = m.lock().unwrap();
        *guard += i;
    }
}

fn trigger_io_in_loop() {
    for i in 0..covopt_param!("M_69_16", 10) {
        println!("This IO call will completely destroy CPU pipeline performance: {}", i);
    }
}
