// Mock implementation of a scheduler lock-free insertion or simple push
// simulating RuinsOS ThreadTask logic to verify O(1) complexity.

use std::collections::VecDeque;
use std::hint::black_box;

pub struct ThreadTask {
    pub id: usize,
    pub priority: u8,
}

#[inline(never)]
pub fn schedule_task(queue: &mut VecDeque<ThreadTask>, task: ThreadTask) {
    // A simple push_back is O(1) amortized.
    // We use black_box to prevent DCE
    queue.push_back(black_box(task));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_o1_complexity() {
        let n: usize = std::env::var("COVOPT_N")
            .unwrap_or_else(|_| "100".to_string())
            .parse()
            .unwrap_or(100);

        let mut queue = VecDeque::new();

        for i in 0..n {
            queue.push_back(ThreadTask { id: i, priority: 1 });
        }

        // We want to test the complexity of this single insertion on a queue of size N
        let task = ThreadTask { id: n, priority: 1 };
        schedule_task(&mut queue, task);

        black_box(queue);
    }
}
