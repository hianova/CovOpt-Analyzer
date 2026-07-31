use std::fs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub async fn async_task_with_blocking_calls() -> String {
    thread::sleep(Duration::from_millis(10));
    fs::read_to_string("dummy.txt").unwrap_or_default()
}

pub async fn async_lock_task(mtx: Arc<Mutex<i32>>) -> i32 {
    *mtx.lock().unwrap()
}
