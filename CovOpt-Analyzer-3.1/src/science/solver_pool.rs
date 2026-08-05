use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct SubprocessPool {
    pool: Arc<Mutex<VecDeque<ProcessWorker>>>,
}

struct ProcessWorker {
    child: Child,
}

impl SubprocessPool {
    pub fn new(command: &str, args: &[&str], size: usize) -> Self {
        let mut workers = VecDeque::new();
        for _ in 0..size {
            let child = Command::new(command)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Failed to spawn process");
            workers.push_back(ProcessWorker { child });
        }
        Self {
            pool: Arc::new(Mutex::new(workers)),
        }
    }

    pub fn execute(&self, payload: &str, end_marker: &str) -> Option<String> {
        let mut worker = {
            let mut pool = self.pool.lock().unwrap();
            loop {
                if let Some(w) = pool.pop_front() {
                    break w;
                }
                drop(pool);
                std::thread::sleep(Duration::from_millis(10));
                pool = self.pool.lock().unwrap();
            }
        };

        let result = Self::interact(&mut worker, payload, end_marker);

        self.pool.lock().unwrap().push_back(worker);
        result
    }

    fn interact(worker: &mut ProcessWorker, payload: &str, end_marker: &str) -> Option<String> {
        {
            let stdin = worker.child.stdin.as_mut()?;
            if stdin.write_all(payload.as_bytes()).is_err() {
                return None;
            }
            if stdin.flush().is_err() {
                return None;
            }
        }

        let stdout = worker.child.stdout.as_mut()?;
        let mut reader = BufReader::new(stdout);
        let mut output = String::new();
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if line.contains(end_marker) {
                        break;
                    }
                    output.push_str(&line);
                }
                Err(_) => return None,
            }
        }
        Some(output)
    }
}

impl Drop for SubprocessPool {
    fn drop(&mut self) {
        let mut pool = self.pool.lock().unwrap();
        for worker in pool.drain(..) {
            let mut w = worker;
            let _ = w.child.kill();
            let _ = w.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subprocess_pool() {
        // Use `cat` as a persistent echo server for testing
        let pool = SubprocessPool::new("cat", &[], 2);

        // cat echoes everything until EOF
        // We will send "hello\nMARKER\n" and expect to read it back
        let result = pool.execute("hello\nMARKER\n", "MARKER");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "hello\n");

        let result2 = pool.execute("world\nMARKER\n", "MARKER");
        assert!(result2.is_some());
        assert_eq!(result2.unwrap(), "world\n");
    }
}
