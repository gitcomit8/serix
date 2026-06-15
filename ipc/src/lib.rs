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
use hal::serial_println;
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
 * @owner_id: Owner identifier
 * @queue: Message queue
 */
pub struct Port {
	id: u64,
	pub owner_id: u64,
	queue: Mutex<VecDeque<Message>>,
	waiting_receivers: Mutex<VecDeque<Arc<spin::Mutex<TaskCB>>>>,
}

impl Port {
	/*
	 * new - Create a new port
	 * @id: Port identifier
	 * @owner_id: Task ID of the port creator
	 *
	 * Return: New Port instance
	 */
	pub fn new(id: u64, owner_id: u64) -> Self {
		Self {
			id,
			owner_id,
			queue: Mutex::new(VecDeque::with_capacity(PORT_QUEUE_LEN)),
			waiting_receivers: Mutex::new(VecDeque::new()),
		}
	}

	/*
	 * id - Get the port identifier
	 *
	 * Return: Port ID
	 */
	pub fn id(&self) -> u64 {
		self.id
	}

	/*
	 * send - Push a message to the port and wake a blocked receiver
	 * @msg: Message to send
	 *
	 * Performs capability validation against the current task's C-space.
	 * If any tasks are blocked waiting for messages on this port,
	 * the first waiter is woken and re-enqueued on the RunQueue.
	 *
	 * Return: Result<(), &'static str> indicating success or failure reason
	 */
	pub fn send(&self, msg: Message) -> Result<(), &'static str> {
		/* 1. Capability Authorization Check */
		let task_arc = task::scheduler::current_task_arc().ok_or("No current task")?;
		let task_cspace = {
			let task = task_arc.lock();
			task.cspace.clone()
		};
		let cap_store = capability::global_cap_store().lock();
		check_send_capability(&task_cspace, self.id, &cap_store)?;

		/* 2. Enqueue Message */
		let mut q = self.queue.lock();
		if q.len() >= PORT_QUEUE_LEN {
			return Err("Port queue full");
		}
		q.push_back(msg);
		drop(q);

		/* 3. Wake first waiting receiver */
		let waiter = self.waiting_receivers.lock().pop_front();
		if let Some(t) = waiter {
			task::wake_task(t);
		}

		Ok(())
	}

	/*
	 * send_kernel - Push a message (Bypasses cap check)
	 *
	 * Intended ONLY for Ring 0 trusted kernel subsystems
	 * Does not require a capability ticker
	 *
	 * Return: Result<(), &'static str> indicating success or failure reason
	 */
	pub fn send_kernel(&self, msg: Message) -> Result<(), &'static str> {
		/* Enqueue Message directly */
		let mut q = self.queue.lock();
		if q.len() >= PORT_QUEUE_LEN {
			return Err("Port queue full");
		}
		q.push_back(msg);
		drop(q);

		/* Wake first waiting receiver, if any */
		let waiter = self.waiting_receivers.lock().pop_front();
		if let Some(t) = waiter {
			task::wake_task(t);
		}
		Ok(())
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
			self.waiting_receivers.lock().push_back(current);

			/* Block and context-switch away */
			task::block_current_and_switch();

			/* Woken up — loop back to retry receive */
		}
	}
}

/*
 * check_send_capability - Verify current task has SEND capability for port
 * @task_cspace: Current task's capability handle table
 * @port_id: Target port ID
 * @cap_store: Global capability store
 *
 * Returns Ok(()) if task has SEND permission for port_id,
 * Err("Port not found") if port doesn't exist,
 * Err("Permission denied") if task lacks capability.
 */
fn check_send_capability(
	task_cspace: &[capability::CapabilityHandle],
	port_id: u64,
	cap_store: &capability::CapabilityStore,
) -> Result<(), &'static str> {
	/* First verify port exists */
	if IPC_GLOBAL.get_port(port_id).is_none() {
		return Err("Port not found");
	}

	/* Check each capability in task's cspace */
	for handle in task_cspace {
		if let Some(cap) = cap_store.get_capability(&handle.key) {
			match cap.cap_type {
				capability::CapabilityType::IpcPort {
					port_id: cap_port_id,
					can_send,
					..
				} => {
					if cap_port_id == port_id && can_send {
						return Ok(());
					}
				}
				_ => {}
			}
		}
	}

	/* Audit log denied attempt */
	let task_id = task::scheduler::current_task_id();
	serial_println!("[AUDIT] ipc: denied send task={} port={}", task_id, port_id);
	Err("Permission denied")
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
	 * create_port - Create a new port and grant SEND capability to creator
	 * @id: Port identifier
	 *
	 * Return: Tuple of (Arc<Port>, CapabilityHandle) for the granted capability
	 *
	 * The capability is automatically added to:
	 * 1. Global capability store
	 * 2. Current task's C-space
	 */
	pub fn create_port(&self, id: u64) -> (Arc<Port>, capability::CapabilityHandle) {
		let mut ports = self.ports.write();
		let owner_id = task::CURRENT_TASK.load(core::sync::atomic::Ordering::Relaxed);
		let port = Arc::new(Port::new(id, owner_id));
		ports.insert(id, port.clone());

		let handle = capability::CapabilityHandle::generate();
		let cap = capability::Capability {
			cap_type: capability::CapabilityType::IpcPort {
				port_id: id,
				can_recv: true,
				can_send: true,
			},
			handle,
		};

		/* Add to global capability store */
		capability::global_cap_store().lock().add_capability(cap);

		/* Add to current task's cspace */
		if let Some(task_arc) = task::scheduler::current_task_arc() {
			task_arc.lock().cspace.push(handle);
		}

		(port, handle)
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
