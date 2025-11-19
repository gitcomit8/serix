/*
 * Task Executor
 *
 * Implements a simple round-robin executor for async tasks.
 */

use crate::async_task::AsyncTask;
use crate::waker::dummy_waker;
use alloc::collections::VecDeque;
use core::task::{Context, Poll};

/*
 * struct Executor - Round-robin async task executor
 * @tasks: Queue of pending tasks
 * @current_task_index: Index of currently executing task
 */
pub struct Executor {
	tasks: VecDeque<AsyncTask>,
	current_task_index: usize,
}

impl Executor {
	/*
	 * new - Create a new empty executor
	 */
	pub fn new() -> Self {
		Self {
			tasks: VecDeque::new(),
			current_task_index: 0,
		}
	}

	/*
	 * spawn - Add a new task to the executor
	 * @task: Task to add to the run queue
	 */
	pub fn spawn(&mut self, task: AsyncTask) {
		self.tasks.push_back(task);
	}

	/*
	 * poll_next_task - Poll the current task once
	 *
	 * Polls one task and advances to the next, removing completed tasks.
	 */
	pub fn poll_next_task(&mut self) {
		if self.tasks.is_empty() {
			return;
		}

		let waker = dummy_waker();
		let mut ctx = Context::from_waker(&waker);

		/* Poll task at the current index */
		if let Some(task) = self.tasks.get_mut(self.current_task_index) {
			match task.poll(&mut ctx) {
				Poll::Ready(()) => {
					/* Remove completed task */
					self.tasks.remove(self.current_task_index);
					if self.current_task_index >= self.tasks.len() && !self.tasks.is_empty() {
						self.current_task_index = 0;
					}
				}
				Poll::Pending => {
					/* Move to next task */
					self.current_task_index = (self.current_task_index + 1) % self.tasks.len();
				}
			}
		}
	}

	/*
	 * poll_all - Poll all tasks once
	 *
	 * Makes one pass through all pending tasks.
	 */
	pub fn poll_all(&mut self) {
		let count = self.tasks.len();
		for _ in 0..count {
			self.poll_next_task();
			if self.tasks.is_empty() {
				break;
			}
		}
	}

	/*
	 * task_yield - Yield to next task
	 *
	 * Advances the task index without polling.
	 */
	pub fn task_yield(&mut self) {
		if !self.tasks.is_empty() {
			self.current_task_index = (self.current_task_index + 1) % self.tasks.len();
		}
	}
}
