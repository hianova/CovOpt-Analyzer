#![allow(unexpected_cfgs)]

use covopt_macro::covopt_bench;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

pub struct AtomicCounter {
    counter: AtomicUsize,
    flag: AtomicBool,
}

impl AtomicCounter {
    pub fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
            flag: AtomicBool::new(false),
        }
    }

    pub fn increment(&self) -> usize {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn get(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }

    pub fn set_flag(&self, val: bool) {
        self.flag.store(val, Ordering::SeqCst);
    }

    pub fn get_flag(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl Default for AtomicCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[covopt_bench]
pub fn dummy_atomic_bench() {
    let counter = Arc::new(AtomicCounter::new());
    let mut handles = vec![];

    for i in 0..4 {
        let c = Arc::clone(&counter);
        let i_val = black_box(i);
        handles.push(thread::spawn(move || {
            c.increment();
            c.set_flag(i_val % 2 == 0);
            let _ = black_box(c.get());
            let _ = black_box(c.get_flag());
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }
}
