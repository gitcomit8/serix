/*
 * scheduler.rs - Run Queue and Scheduler Infrastructure
 *
 * Provides the RunQueue type for managing runnable tasks on a single CPU
 * This is infrastructure only - timer wiring and context switch integration
 * will happen later
 *
 * TODO(SMP): Replace global RunQueue with per-CPU run queues indexed via GS_BASE
 */

use super::{CURRENT_TASK, TaskCB, TaskState};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;
use spin::{Mutex, Once};

/*
 * TIME_SLICE_TICKS - Number of timer ticks per scheduling quantum
 *
 * Each task runs for this many ticks before the scheduler is invoked.
 * At ~625 Hz timer frequency (100_00 initial count / 16 divider),
 * 10 ticks = 16 ms per time slice.
 *
 * TODO(SMP): May need per-CPU adjustment for load balancing
 */
pub const TIME_SLICE_TICKS: u64 = 10;

/*
 * struct RunQueue - Single-CPU run queue for ready tasks
 * @queue: Deque fo tasks ready to run (front = next to run)
 * @current: Currently running task (None during early boot)
 *
 * Holds Arc<Mutex<TaskCB>> so tasks have stable heap addresses
 * regardless of queue reordering. The Mutex allows state mutation
 * (Ready <-> Running) under the run queue lock
 *
 * TODO(SMP): Per-CPU run queues with GS_BASE
 */
pub struct RunQueue {
	queue: VecDeque<Arc<Mutex<TaskCB>>>,
	current: Option<Arc<Mutex<TaskCB>>>,
}

/* Global single-CPU run queue */
static RUN_QUEUE: Once<Mutex<RunQueue>> = Once::new();

impl RunQueue {
	/*
	 * new - Create an empty run queue
	 *
	 * Return: New RunQueue instance
	 */
	fn new() -> Self {
		RunQueue {
			queue: VecDeque::new(),
			current: None,
		}
	}

	/*
	 * enqueue - Add a task to the back of run queue
	 * @task: Arc-wrapped task to enqueue
	 *
	 * Sets task state to Ready before inserting. The task will be
	 * selected by dequeue() in FIFO order
	 *
	 * Safety: Caller must hold RunQueue lock
	 */
	pub fn enqueue(&mut self, task: Arc<Mutex<TaskCB>>) {
		task.lock().set_state(TaskState::Ready);
		self.queue.push_back(task);
	}

	/*
	 * dequeue - Remove and return the next runnable task
	 *
	 * Pops from the front of the queue. Caller is responsible for
	 * transitioning the returned task to Running state.
	 *
	 * Return: Some(task) if queue is non-empty, None otherwise
	 *
	 * Safety: Caller must hold the RunQueue lock.
	 */
	pub fn dequeue(&mut self) -> Option<Arc<Mutex<TaskCB>>> {
		self.queue.pop_front()
	}

	/*
	 * peek - Inspect the next task without removing it
	 *
	 * Return: Some(task) reference if queue is non-empty, None otherwise
	 *
	 * Safety: Caller must hold the RunQueue lock
	 */
	pub fn peek(&self) -> Option<&Arc<Mutex<TaskCB>>> {
		self.queue.front()
	}
	/*
	 * is_empty - Check whether the run queue has no tasks
	 *
	 * Return: true if no tasks are queued
	 */
	pub fn is_empty(&self) -> bool {
		self.queue.is_empty()
	}

	/*
	 * len - Number of tasks waiting in the run queue
	 *
	 * Return: Count of queued (not yet running) tasks
	 */
	pub fn len(&self) -> usize {
		self.queue.len()
	}
}

/*
 * init - Initialize the global run queue
 *
 * Must be called once during kernel startup before any tasks are enqueued.
 * Subsequent calls are no-ops (spin::Once guarantees single init).
 */
pub fn init() {
	RUN_QUEUE.call_once(|| Mutex::new(RunQueue::new()));
}

/*
 * global - Get reference to the global run queue
 *
 * Panics if init() has not been called.
 *
 * Return: Reference to the global Mutex<RunQueue>
 */
pub fn global() -> &'static Mutex<RunQueue> {
	RUN_QUEUE
		.get()
		.expect("RunQueue not initialized — call scheduler::init() first")
}

/*
 * enqueue_task - Enqueue a task into the global run queue
 * @task: Arc-wrapped task to enqueue
 *
 * Convenience wrapper around global().lock().enqueue().
 */
pub fn enqueue_task(task: Arc<Mutex<TaskCB>>) {
	global().lock().enqueue(task);
}

/*
 * current_task_id - Get the task ID of the currently running task
 *
 * Return: TaskId value, or 0 if no task is running
 */
pub fn current_task_id() -> u64 {
	CURRENT_TASK.load(Ordering::Acquire)
}
