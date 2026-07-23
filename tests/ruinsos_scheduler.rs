use covopt_macro::covopt_param;
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

use covopt_macro::covopt_test;

#[cfg(test)]
#[covopt_test(target_fn = "schedule_task", expected = "O1")]
fn ruinsos_scheduler(n: usize) {
    let mut sum = 0;
    // O(N) Dummy Initialization Loop
    // Dominant Complexity Auto-Detection should completely ignore this loop
    for i in 0..n {
        sum += i;
        std::hint::black_box(sum);
    }

    let mut queue = VecDeque::new();

    for i in std::hint::black_box(0..n) {
        queue.push_back(ThreadTask { id: i, priority: 1 });
    }

    // We want to test the complexity of this single insertion on a queue of size N
    let task = ThreadTask { id: n, priority: 1 };
    schedule_task(&mut queue, task);

    black_box(queue);
}
