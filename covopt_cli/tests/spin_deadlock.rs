use covopt_macro::covopt_param;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpinMutex {
    state: AtomicUsize,
}

impl SpinMutex {
    pub fn new(val: usize) -> Self {
        Self {
            state: AtomicUsize::new(val),
        }
    }

    pub fn lock(&self) -> SpinMutexGuard<'_> {
        let mut spins = 0;
        while self
            .state
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spins += 1;
            if spins > covopt_param!("M_24_23", 10000) {
                std::thread::yield_now();
            } else {
                std::hint::spin_loop();
            }
        }
        SpinMutexGuard { mutex: self }
    }
}

pub struct SpinMutexGuard<'a> {
    mutex: &'a SpinMutex,
}

impl<'a> Drop for SpinMutexGuard<'a> {
    fn drop(&mut self) {
        self.mutex.state.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spin_mutex_heavy_contention() {
        let default_n_str = covopt_param!("SPIN_DEFAULT_N_STR", "100".to_string());
        let n: usize = std::env::var("COVOPT_N")
            .unwrap_or_else(|_| default_n_str.to_string())
            .parse()
            .unwrap_or(covopt_param!("M_54_23", 100));

        let mutex = Arc::new(SpinMutex::new(0));
        let mut handles = vec![];

        for _ in black_box(0..n) {
            let m = mutex.clone();
            handles.push(std::thread::spawn(move || {
                for _ in black_box(0..covopt_param!("M_62_49", 1000)) {
                    let _guard = m.lock();
                    black_box(1);
                }
            }));
        }

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for handle in black_box(handles) {
                handle.join().unwrap();
            }
            tx.send(()).unwrap();
        });

        let timeout_secs = covopt_param!("SPIN_TIMEOUT_SECS", 5);
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(timeout_secs))
                .is_ok(),
            "Detected Spin Deadlock or extreme starvation under high contention!"
        );

        black_box(n);
    }
}
