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
use core::sync::atomic::{AtomicU64, Ordering};
use hal::serial_println;
use spin::Mutex;
use spin::lock_api::RwLock;
use task::TaskCB;

/*
 * IPC Constants
 */
pub use ipc_types::MAX_MSG_SIZE;
pub use ipc_types::Message;
pub const PORT_QUEUE_LEN: usize = 32;

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
	/* Notification bitmask for async notification ports */
	notification_bitmask: AtomicU64,
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
			notification_bitmask: AtomicU64::new(0),
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
	 *
	 * Fastpath: If a task is blocked waiting on this port, directly inject
	 * the message into its TaskCB (bypassing the queue) and wake it.
	 * This avoids the extra wake/schedule cycle.
	 *
	 * Slowpath: Enqueue the message and wake the first blocked receiver
	 * via the normal run queue mechanism.
	 *
	 * Return: Result<(), &'static str> indicating success or failure reason
	 */
	pub fn send(&self, msg: Message) -> Result<(), &'static str> {
		/* 1. Capability Authorization Check */
		let task_arc = task::scheduler::current_task_arc().ok_or("No current task")?;
		let task_cspace = {
			let task = task_arc.lock();
			Arc::clone(&task.cspace)
		};
		let cap_store = capability::global_cap_store().lock();
		let cspace_guard = task_cspace.lock();
		check_send_capability(&cspace_guard, self.id, &cap_store)?;

		/* 2. Try fastpath: check for blocked receiver */
		let waiter = self.waiting_receivers.lock().pop_front();
		if let Some(receiver) = waiter {
			/* Fastpath: inject message directly into receiver's TaskCB */
			let msg_arc = Arc::new(msg);
			task::inject_direct_message(&receiver, msg_arc);
			task::wake_task(receiver);
			return Ok(());
		}

		/* 3. Slowpath: enqueue message */
		let mut q = self.queue.lock();
		if q.len() >= PORT_QUEUE_LEN {
			return Err("Port queue full");
		}
		q.push_back(msg);
		drop(q);

		/* 4. Wake first waiting receiver (if any arrived between our checks) */
		let waiter = self.waiting_receivers.lock().pop_front();
		if let Some(t) = waiter {
			task::wake_task(t);
		}

		Ok(())
	}

	/*
	 * send_kernel - Push a message (Bypasses cap check)
	 *
	 * Intended ONLY for Ring 0 trusted kernel subsystems.
	 * Does not require a capability ticker.
	 *
	 * Fastpath: If a task is blocked waiting on this port, directly inject
	 * the message into its TaskCB (bypassing the queue) and wake it.
	 *
	 * Slowpath: Enqueue the message and wake the first blocked receiver.
	 *
	 * Return: Result<(), &'static str> indicating success or failure reason
	 */
	pub fn send_kernel(&self, msg: Message) -> Result<(), &'static str> {
		/* Try fastpath: check for blocked receiver */
		let waiter = self.waiting_receivers.lock().pop_front();
		if let Some(receiver) = waiter {
			/* Fastpath: inject message directly into receiver's TaskCB */
			let msg_arc = Arc::new(msg);
			task::inject_direct_message(&receiver, msg_arc);
			task::wake_task(receiver);
			return Ok(());
		}

		/* Slowpath: enqueue message */
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
	 * notify - Set notification bitmask and wake blocked receiver
	 *
	 * Used by async notification ports. Sets the event bitmask via atomic OR,
	 * then wakes the first blocked receiver (same mechanism as the IPC fastpath).
	 * The receiver will see the bitmask via check_notification().
	 *
	 * @bitmask: Event bitmask to set
	 */
	pub fn notify(&self, bitmask: u64) {
		self.notification_bitmask.fetch_or(bitmask, Ordering::Relaxed);

		/* Wake first blocked receiver, if any */
		let waiter = self.waiting_receivers.lock().pop_front();
		if let Some(receiver) = waiter {
			task::wake_task(receiver);
		}
	}

	/*
	 * check_notification - Read and clear the notification bitmask
	 *
	 * Returns the current bitmask and clears it atomically.
	 * Called by receive_blocking() to check for async notifications
	 * before falling back to the message queue.
	 *
	 * Return: Notification bitmask (0 if none pending)
	 */
	pub fn check_notification(&self) -> u64 {
		self.notification_bitmask.swap(0, Ordering::Relaxed)
	}

	/*
	 * is_notification_port - Check if this port is an async notification port
	 *
	 * Return: true if the port was created as a notification port
	 */
	pub fn is_notification_port(&self) -> bool {
		/* Notification ports have a non-zero initial bitmask set by create_notification_port */
		false /* Default ports are not notification ports; set by create_notification_port */
	}

	/*
	 * receive_blocking - Block until a message is available
	 *
	 * If the queue is empty, places the current task on the wait queue,
	 * blocks it, and retries upon waking. Handles spurious wakes by
	 * looping until a message is actually available.
	 *
	 * Fastpath: Checks for a direct message injected by the IPC fastpath
	 * (send() when receiver is blocked). This bypasses the queue entirely.
	 *
	 * Priority inheritance: if a previous sender is identified via
	 * the message header, the sender's priority is boosted to match the
	 * blocking receiver's priority to prevent priority inversion.
	 *
	 * Return: The received Message
	 *
	 * Safety: Must be called with interrupts disabled.
	 *         Must not be called from interrupt context.
	 */
	pub fn receive_blocking(&self) -> Message {
		loop {
			/* Fastpath 1: direct message injected by send() fastpath */
			if let Some(msg) = task::consume_direct_message() {
				/* Apply priority inheritance to sender if available */
				if msg.sender_id != 0 {
					if let Some(sender) = task::scheduler::find_task_by_id(msg.sender_id) {
						let receiver = task::current_task_arc().unwrap();
						let recv_prio = receiver.lock().priority();
						let sender_prio = sender.lock().priority();
						if sender_prio < recv_prio {
							task::scheduler::boost_priority(&sender, recv_prio);
						}
					}
				}
				return msg;
			}

			/* Fastpath 1.5: async notification bitmask */
			let notif = self.check_notification();
			if notif != 0 {
				/* Notification received — return a synthetic message with the bitmask */
				return Message {
					sender_id: 0,
					id: notif,
					len: 0,
					data: [0u8; MAX_MSG_SIZE],
				};
			}

			/* Fastpath 2: message already available in queue */
			if let Some(msg) = self.queue.lock().pop_front() {
				/* Apply priority inheritance to sender if available */
				if msg.sender_id != 0 {
					if let Some(sender) = task::scheduler::find_task_by_id(msg.sender_id) {
						let receiver = task::current_task_arc().unwrap();
						let recv_prio = receiver.lock().priority();
						let sender_prio = sender.lock().priority();
						if sender_prio < recv_prio {
							task::scheduler::boost_priority(&sender, recv_prio);
						}
					}
				}
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
	task_cspace: &capability::cspace::CapabilitySpace,
	port_id: u64,
	cap_store: &capability::CapabilityStore,
) -> Result<(), &'static str> {
	/* First verify port exists */
	if IPC_GLOBAL.get_port(port_id).is_none() {
		return Err("Port not found");
	}

	/* Check each capability slot in task's cspace */
	for (slot, cap_id, _gen) in task_cspace.iter() {
		if let Some(record) = cap_store.lookup(cap_id) {
			match record.cap_type {
				capability::CapabilityType::IpcPort {
					port_id: cap_port_id,
					can_send,
					..
				} if cap_port_id == port_id && can_send => {
					/* Verify record is active (not revoked/expired) */
					if record.is_active(0) {
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
		drop(ports);

		let handle = capability::CapabilityHandle::generate();
		let cap_type = capability::CapabilityType::IpcPort {
			port_id: id,
			can_recv: true,
			can_send: true,
		};
		let rights = capability::Rights::SEND | capability::Rights::RECV;

		/* Create record and insert into global store */
		let record = capability::global_cap_store()
			.lock()
			.mint_root(handle, cap_type, rights, 0);

		/* Insert into current task's cspace */
		if let Some(task_arc) = task::scheduler::current_task_arc() {
			task_arc.lock().cspace.lock().insert(record.id, false);
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

	/*
	 * create_notification_port - Create an async notification port
	 * @id: Port identifier
	 * @bitmask: Initial event bitmask to listen for
	 *
	 * Creates a port that supports async notifications via notify()/check_notification().
	 * Grants a CapabilityType::AsyncNotification handle to the creator.
	 *
	 * Return: Tuple of (Arc<Port>, CapabilityHandle) for the notification capability
	 */
	pub fn create_notification_port(&self, id: u64, bitmask: u64) -> (Arc<Port>, capability::CapabilityHandle) {
		let mut ports = self.ports.write();
		let owner_id = task::CURRENT_TASK.load(core::sync::atomic::Ordering::Relaxed);
		let port = Arc::new(Port::new(id, owner_id));
		/* Set initial notification bitmask */
		port.notification_bitmask.store(bitmask, Ordering::Relaxed);
		ports.insert(id, port.clone());
		drop(ports);

		let handle = capability::CapabilityHandle::generate();
		let cap_type = capability::CapabilityType::AsyncNotification { port_id: id };
		let rights = capability::Rights::NOTIFY;

		/* Create record and insert into global store */
		let record = capability::global_cap_store()
			.lock()
			.mint_root(handle, cap_type, rights, 0);

		/* Insert into current task's cspace */
		if let Some(task_arc) = task::scheduler::current_task_arc() {
			task_arc.lock().cspace.lock().insert(record.id, false);
		}

		(port, handle)
	}
}

/*
 * Global IPC Space
 */
pub static IPC_GLOBAL: IpcSpace = IpcSpace::new();
