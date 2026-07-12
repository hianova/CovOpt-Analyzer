use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::hint::black_box;

pub struct SpinMutex {
    state: AtomicUsize,
}

impl SpinMutex {
    pub fn new(val: usize) -> Self {
        Self {
            state: AtomicUsize::new(val),
        }
    }

    pub fn lock(&self) -> SpinMutexGuard {
        let mut spins = 0;
        while self.state.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
            spins += 1;
            if spins > 10_000 {
                // Spin limit exceeded, fallback or log
                std::hint::spin_loop();
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
        let n: usize = std::env::var("COVOPT_N")
            .unwrap_or_else(|_| "100".to_string())
            .parse()
            .unwrap_or(100);

        let mutex = Arc::new(SpinMutex::new(0));
        let mut handles = vec![];

        for _ in 0..n {
            let m = mutex.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..1_000 {
                    let _guard = m.lock();
                    black_box(1);
                }
            }));
        }

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for handle in handles {
                handle.join().unwrap();
            }
            tx.send(()).unwrap();
        });

        // Watchdog Timeout: Fail the test if it spins too long
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "Detected Spin Deadlock or extreme starvation under high contention!"
        );
        
        // This is our target line for O(N) threads
        black_box(n);
    }
}
