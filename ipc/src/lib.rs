#![no_std]
extern crate alloc;
use alloc::collections::VecDeque;
use spin::Mutex;

pub struct Message {
	pub from_id: u64,
	pub content: u64,
}

pub struct Mailbox {
	queue: Mutex<VecDeque<Message>>,
}

impl Mailbox {
	pub fn new() -> Self {
		Self {
			queue: Mutex::new(VecDeque::new()),
		}
	}
	pub fn send(&self, msg: Message) {
		self.queue.lock().push_back(msg);
	}
	pub fn receive(&self) -> Option<Message> {
		self.queue.lock().pop_front()
	}
}
