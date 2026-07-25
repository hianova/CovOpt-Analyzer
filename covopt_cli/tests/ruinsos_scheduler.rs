use covopt_macro::covopt_param;
use std::collections::VecDeque;
use std::hint::black_box;

#[repr(C, align(64))]
pub struct ThreadTask {
    pub id: usize,
    pub priority: u8,
}

#[inline(never)]
pub fn schedule_task(queue: &mut VecDeque<ThreadTask>, task: ThreadTask) {
    queue.push_back(black_box(task));
}

use covopt_macro::covopt_test;

#[cfg(test)]
#[covopt_test(target_fn = "schedule_task", expected = "O(1)", n_values = "1,100,1000")]
fn ruinsos_scheduler(n: usize) {
    let mut sum = 0;
    for i in black_box(0..n) {
        sum += i;
        black_box(sum);
    }

    let mut queue = VecDeque::new();
    let task_priority: u8 = covopt_param!("RUINSOS_TASK_PRIORITY", 1);
    for i in black_box(0..n) {
        queue.push_back(ThreadTask {
            id: i,
            priority: task_priority,
        });
    }

    let task = ThreadTask {
        id: n,
        priority: task_priority,
    };
    schedule_task(&mut queue, task);

    black_box(queue);
}
