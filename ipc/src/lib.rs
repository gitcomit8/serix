/*
 * Pulse IPC Subsystem
 *
 * Implements a port-based message passing system
 * Supports synchronous (blocking) and asynchronous (non-blocking) modes
 */

#![no_std]
extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use spin::lock_api::RwLock;
use spin::Mutex;
/*
 * IPC Constants
 */

pub const MAX_MSG_SIZE: usize = 128;
pub const PORT_QUEUE_LEN: usize = 32;

/*
 * struct Message - Standard IPC message format
 * Fits in registers or small stack buffer
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
 */
pub struct Port {
	id: u64,
	queue: Mutex<VecDeque<Message>>,
	//TODO: waiting_tasks: Mutex<Vec<TaskId>>,
}

impl Port {
	pub fn new(id: u64) -> Self {
		Self {
			id,
			queue: Mutex::new(VecDeque::with_capacity(PORT_QUEUE_LEN)),
		}
	}

	/*
	 * send - Push a message to the port
	 * Returns true if successful, false if queue full
	 */
	pub fn send(&self, msg: Message) -> bool {
		let mut q = self.queue.lock();
		if q.len() >= PORT_QUEUE_LEN {
			return false;
		}
		q.push_back(msg);
		//TODO: Wake up waiting tasks
		true
	}

	/*
	 * receive - Pop a message from the port
	 * Returns Some(msg) or None if empty
	 */
	pub fn receive(&self) -> Option<Message> {
		let mut q = self.queue.lock();
		q.pop_front()
	}
}

/*
 * struct IpcSpace - IPC Namespace (Gloabl for now)
 */
pub struct IpcSpace {
	ports: RwLock<BTreeMap<u64, Arc<Port>>>,
}

impl IpcSpace {
	pub const fn new() -> Self {
		Self {
			ports: RwLock::new(BTreeMap::new()),
		}
	}

	pub fn create_port(&self, id: u64) -> Arc<Port> {
		let mut ports = self.ports.write();
		let port = Arc::new(Port::new(id));
		ports.insert(id, port.clone());
		port
	}

	pub fn get_port(&self, id: u64) -> Option<Arc<Port>> {
		let ports = self.ports.read();
		ports.get(&id).cloned()
	}
}

/* Global IPC Space */
pub static IPC_GLOBAL: IpcSpace = IpcSpace::new();
