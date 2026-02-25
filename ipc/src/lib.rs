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
 * @waiting_tasks: Task IDs waiting to receive a message
 */
pub struct Port {
	id: u64,
	queue: Mutex<VecDeque<Message>>,
	waiting_tasks: Mutex<VecDeque<u64>>,
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
			waiting_tasks: Mutex::new(VecDeque::new()),
		}
	}

	/*
	 * send - Push a message to the port
	 * @msg: Message to send
	 *
	 * Return: true if successful, false if queue full
	 */
	pub fn send(&self, msg: Message) -> bool {
		let mut q = self.queue.lock();
		if q.len() >= PORT_QUEUE_LEN {
			return false;
		}
		q.push_back(msg);
		/* Wake up the first waiting task by removing it from the list */
		let _ = self.waiting_tasks.lock().pop_front();
		true
	}

	/*
	 * receive - Pop a message from the port (non-blocking)
	 *
	 * Return: Some(msg) or None if empty
	 */
	pub fn receive(&self) -> Option<Message> {
		let mut q = self.queue.lock();
		q.pop_front()
	}

	/*
	 * receive_blocking - Block until a message is available
	 * @task_id: ID of the calling task (registered as a waiter)
	 *
	 * Spins until a message arrives in the port queue.
	 * Return: The received message
	 */
	pub fn receive_blocking(&self, task_id: u64) -> Message {
		/* Register this task as waiting */
		self.waiting_tasks.lock().push_back(task_id);

		/* Spin until a message is available */
		loop {
			if let Some(msg) = self.receive() {
				return msg;
			}
			core::hint::spin_loop();
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
