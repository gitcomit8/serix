/*
 * lib.rs - Pulse IPC Subsystem
 *
 * Implements a port-based message passing system.
 * Supports synchronous (blocking) and asynchronous (non-blocking) modes.
 */

#![no_std]
extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use spin::Mutex;
use spin::lock_api::RwLock;
use task::TaskCB;

/*
 * IPC Constants
 */
pub const MAX_MSG_SIZE: usize = 128;
pub const PORT_QUEUE_LEN: usize = 32;

/*
 * struct Message - Standard IPC message format
 * @sender_id: Sender task ID
 * @id: Message ID/type
 * @len: Message data length
 * @data: Message payload
 *
 * Fits in registers or small stack buffer.
 */
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Message {
	pub sender_id: u64,
	pub id: u64,
	pub len: u64,
	pub data: [u8; MAX_MSG_SIZE],
}

impl Default for Message {
	fn default() -> Self {
		Self {
			sender_id: 0,
			id: 0,
			len: 0,
			data: [0; MAX_MSG_SIZE],
		}
	}
}

/*
 * struct Port - Communication port
 * @id: Port identifier
 * @queue: Message queue
 */
pub struct Port {
	id: u64,
	queue: Mutex<VecDeque<Message>>,
	waiting_receivers: Mutex<VecDeque<Arc<Mutex<TaskCB>>>>,
}

impl Port {
	/*
	 * new - Create a new port
	 * @id: Port identifier
	 *
	 * Return: New Port instance
	 */
	pub fn new(id: u64) -> Self {
		Self {
			id,
			queue: Mutex::new(VecDeque::with_capacity(PORT_QUEUE_LEN)),
			waiting_receivers: Mutex::new(VecDeque::new()),
		}
	}

	/*
	 * send - Push a message to the port and wake a blocked receiver
	 * @msg: Message to send
	 *
	 * If any tasks are blocked waiting for messages on this port,
	 * the first waiter is woken and re-enqueued on the RunQueue.
	 *
	 * Return: true if successful, false if queue full
	 */
	pub fn send(&self, msg: Message) -> bool {
		let mut q = self.queue.lock();
		if q.len() >= PORT_QUEUE_LEN {
			return false;
		}
		q.push_back(msg);
		drop(q);

		/* Wake first waiting receiver, if any */
		let waiter = self.waiting_receivers.lock().pop_front();
		if let Some(t) = waiter {
			task::wake_task(t);
		}
		true
	}

	/*
	 * receive - Pop a message from the port
	 *
	 * Return: Some(msg) or None if empty
	 */
	pub fn receive(&self) -> Option<Message> {
		let mut q = self.queue.lock();
		q.pop_front()
	}

	/*
	 * receive_blocking - Block until a message is available
	 *
	 * If the queue is empty, places the current task on the wait queue,
	 * blocks it, and retries upon waking. Handles spurious wakes by
	 * looping until a message is actually available.
	 *
	 * Return: The received Message
	 *
	 * Safety: Must be called with interrupts disabled.
	 *         Must not be called from interrupt context.
	 */
	pub fn receive_blocking(&self) -> Message {
		loop {
			/* Fast path: message already available */
			if let Some(msg) = self.queue.lock().pop_front() {
				return msg;
			}

			/* Queue empty — block current task */
			let current = match task::current_task_arc() {
				Some(arc) => arc,
				None => {
					core::hint::spin_loop();
					continue;
				}
			};

			/* Place on wait queue BEFORE removing from RunQueue */
			self.waiting_receivers.lock().push_back(
				Arc::clone(&current),
			);

			/* Block and context-switch away */
			task::block_current_and_switch();

			/* Woken up — loop back to retry receive */
		}
	}
}

/*
 * struct IpcSpace - IPC Namespace (Global for now)
 * @ports: Map of port IDs to port objects
 */
pub struct IpcSpace {
	ports: RwLock<BTreeMap<u64, Arc<Port>>>,
}

impl IpcSpace {
	/*
	 * new - Create a new IPC namespace
	 *
	 * Return: New IpcSpace instance
	 */
	pub const fn new() -> Self {
		Self {
			ports: RwLock::new(BTreeMap::new()),
		}
	}

	/*
	 * create_port - Create a new port
	 * @id: Port identifier
	 *
	 * Return: Arc reference to the new port
	 */
	pub fn create_port(&self, id: u64) -> Arc<Port> {
		let mut ports = self.ports.write();
		let port = Arc::new(Port::new(id));
		ports.insert(id, port.clone());
		port
	}

	/*
	 * get_port - Get an existing port
	 * @id: Port identifier
	 *
	 * Return: Some(port) if found, None otherwise
	 */
	pub fn get_port(&self, id: u64) -> Option<Arc<Port>> {
		let ports = self.ports.read();
		ports.get(&id).cloned()
	}
}

/*
 * Global IPC Space
 */
pub static IPC_GLOBAL: IpcSpace = IpcSpace::new();
