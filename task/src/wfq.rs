/*
 * Weighted Fair Queueing (WFQ) Scheduler
 *
 * Implements priority-weighted fair scheduling for the `Fair` scheduling
 * class. Tasks with lower virtual runtime are picked first, giving
 * higher-priority tasks (lower numeric priority = higher weight) more
 * CPU time proportionally.
 *
 * RT tasks (SchedClass::Realtime) always run first regardless of virtual
 * runtime.
 *
 * Virtual runtime is incremented on each timer tick:
 *   vruntime += tick_duration_ns * 100 / weight
 *
 * Weight mapping (Fair priority → weight):
 *   priority 100 → weight 1 (lowest weight = runs most)
 *   priority 130 → weight 31
 *   priority 139 → weight 40
 */

use crate::{SchedClass, TaskCB};
use alloc::sync::Arc;
use spin::Mutex;

/*
 * WFQ_TICK_DURATION_NS - Approximate duration of one timer tick in nanoseconds
 *
 * At ~625 Hz (100_000 initial count / 16 divider),
 * 1 tick ≈ 1600 ns.
 */
const WFQ_TICK_DURATION_NS: u64 = 1600;

/*
 * wfq_weight - Compute WFQ weight from Fair priority
 * @priority: Task priority (100-139 for Fair class)
 *
 * Returns weight where lower priority number = higher weight.
 * Priority 100 → weight 40, priority 139 → weight 1.
 */
pub fn wfq_weight(priority: u8) -> u64 {
	if priority <= 100 {
		40
	} else {
		(140u8.saturating_sub(priority)) as u64
	}
}

/*
 * pick_next_wfq - Select the next task to run using WFQ
 *
 * RT tasks always run first. Among Fair tasks, the one with the lowest
 * virtual runtime is selected.
 *
 * @tasks: Slice of ready tasks on the current CPU
 *
 * Return: Some(Arc) of the selected task, None if queue is empty
 */
pub fn pick_next_wfq(tasks: &[Arc<Mutex<TaskCB>>]) -> Option<Arc<Mutex<TaskCB>>> {
	/* Preserve the fixed class ordering before applying WFQ inside Fair. */
	for task in tasks {
		if matches!(task.lock().sched_class, SchedClass::Realtime(_)) {
			return Some(Arc::clone(task));
		}
	}
	for task in tasks {
		if matches!(task.lock().sched_class, SchedClass::Iso) {
			return Some(Arc::clone(task));
		}
	}
	let mut best: Option<Arc<Mutex<TaskCB>>> = None;
	let mut min_vruntime: u64 = u64::MAX;

	for task in tasks {
		let t = task.lock();

		/* Fair is ordered by virtual runtime; Batch is the final fallback. */
		match t.sched_class {
			SchedClass::Batch => continue,
			SchedClass::Fair(_) => {}
			SchedClass::Realtime(_) | SchedClass::Iso => continue,
		}

		/* Fair tasks: pick lowest virtual runtime */
		if t.virtual_runtime < min_vruntime {
			min_vruntime = t.virtual_runtime;
			best = Some(Arc::clone(task));
		}
	}

	best.or_else(|| tasks.iter().find(|task| matches!(task.lock().sched_class, SchedClass::Batch)).cloned())
}

/*
 * update_virtual_runtime - Increment virtual runtime for the given task
 *
 * Called from the timer interrupt handler when a task's time slice expires.
 *
 * @task: Task whose virtual runtime to update
 */
pub fn update_virtual_runtime(task: &mut TaskCB) {
	match task.sched_class {
		SchedClass::Fair(_) => {
			let weight = wfq_weight(task.priority());
			task.virtual_runtime += WFQ_TICK_DURATION_NS * 100 / weight;
		}
		SchedClass::Batch | SchedClass::Iso | SchedClass::Realtime(_) => {}
	}
}

/*
 * pick_next - Alias for pick_next_wfq (used by scheduler)
 *
 * @tasks: Slice of ready tasks
 *
 * Return: Some(task) if a runnable task exists, None if queue is empty
 */
pub fn pick_next(tasks: &[Arc<Mutex<TaskCB>>]) -> Option<Arc<Mutex<TaskCB>>> {
	pick_next_wfq(tasks)
}
