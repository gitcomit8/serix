use crate::async_task::AsyncTask;
use crate::waker::dummy_waker;
use alloc::collections::VecDeque;
use core::task::{Context, Poll};

pub struct Executor {
	tasks: VecDeque<AsyncTask>,
	current_task_index: usize,
}

impl Executor {
	pub fn new() -> Self {
		Self {
			tasks: VecDeque::new(),
			current_task_index: 0,
		}
	}

	pub fn spawn(&mut self, task: AsyncTask) {
		self.tasks.push_back(task);
	}

	pub fn poll_next_task(&mut self) {
		if self.tasks.is_empty() {
			return;
		}

		let waker = dummy_waker();
		let mut ctx = Context::from_waker(&waker);

		// Poll task at the current index
		if let Some(task) = self.tasks.get_mut(self.current_task_index) {
			match task.poll(&mut ctx) {
				Poll::Ready(()) => {
					// Remove completed task
					self.tasks.remove(self.current_task_index);
					if self.current_task_index >= self.tasks.len() && !self.tasks.is_empty() {
						self.current_task_index = 0;
					}
				}
				Poll::Pending => {
					// Move to next task for next poll
					self.current_task_index = (self.current_task_index + 1) % self.tasks.len();
				}
			}
		}
	}

	pub fn poll_all(&mut self) {
		let count = self.tasks.len();
		for _ in 0..count {
			self.poll_next_task();
			if self.tasks.is_empty() {
				break;
			}
		}
	}

	pub fn task_yield(&mut self) {
		if !self.tasks.is_empty() {
			self.current_task_index = (self.current_task_index + 1) % self.tasks.len();
		}
	}
}
