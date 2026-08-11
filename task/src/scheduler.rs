/*
 * scheduler.rs - Per-CPU Run Queue and Scheduler Infrastructure
 *
 * Each logical processor maintains its own run queue, eliminating
 * cross-core lock contention and enabling true SMP scheduling.
 * The per-CPU data area is pointed to by the GS_BASE MSR.
 */

use super::{CURRENT_TASK, SchedClass, TaskCB, TaskState};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use spin::{Mutex, Once};

/*
 * TIME_SLICE_TICKS - Number of timer ticks per scheduling quantum
 *
 * At ~625 Hz timer frequency (100_000 initial count / 16 divider),
 * 10 ticks = 16 ms per time slice.
 */
pub const TIME_SLICE_TICKS: u64 = 10;

/*
 * struct RunQueue - Per-CPU run queue for ready tasks
 * @cpu_id: ID of the CPU that owns this queue
 * @queue: Deque of tasks ready to run (front = next to run)
 * @current: Currently running task on this CPU
 * @zombies: Tasks that have exited but not yet reaped by wait4
 *
 * Holds Arc<Mutex<TaskCB>> so tasks have stable heap addresses
 * regardless of queue reordering. The Mutex allows state mutation
 * (Ready <-> Running) under the run queue lock.
 */
pub struct RunQueue {
	queue: VecDeque<Arc<Mutex<TaskCB>>>,
	pub current: Option<Arc<Mutex<TaskCB>>>,
	pub zombies: Vec<Arc<Mutex<TaskCB>>>,
}

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
			zombies: Vec::new(),
		}
	}

	/*
	 * enqueue - Add a task to the back of run queue
	 * @task: Arc-wrapped task to enqueue
	 *
	 * Sets task state to Ready before inserting. The task will be
	 * selected by dequeue() in FIFO order.
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

	fn dequeue_scheduled(&mut self) -> Option<Arc<Mutex<TaskCB>>> {
		let tasks: Vec<_> = self.queue.iter().cloned().collect();
		let selected = crate::wfq::pick_next(&tasks)?;
		let index = self.queue.iter().position(|task| Arc::ptr_eq(task, &selected))?;
		self.queue.remove(index)
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

/* Per-CPU run queue array, one entry per CPU (max 16) */
static PER_CPU_RUN_QUEUES: Once<[Once<Mutex<RunQueue>>; 16]> = Once::new();

/* Per-CPU data area (set by kernel during init) */
static mut PER_CPU_DATA_BASE: usize = 0;

/*
 * init - Initialize per-CPU scheduling infrastructure
 * @per_cpu_data_base: Virtual address of the per-CPU data array
 * @cpu_id: ID of the current CPU (0 = BSP)
 *
 * Sets up the per-CPU run queue and records the PerCpuData base
 * address so context switch code can find it via GS_BASE.
 */
pub fn init(per_cpu_data_base: usize, cpu_id: u8) {
	PER_CPU_RUN_QUEUES.call_once(|| {
		let mut arr = [const { Once::new() }; 16];
		arr
	});
	/* Each CPU initializes its own run queue slot */
	PER_CPU_RUN_QUEUES.get().unwrap()[cpu_id as usize].call_once(|| Mutex::new(RunQueue::new()));
	unsafe {
		PER_CPU_DATA_BASE = per_cpu_data_base;
		// Point PerCpuData.run_queue (offset 40) at the per-CPU Mutex<RunQueue>
		let queues_arr = PER_CPU_RUN_QUEUES.get().unwrap();
		let rq_ptr = &*queues_arr[cpu_id as usize].get().unwrap() as *const Mutex<RunQueue> as usize;
		core::ptr::write_volatile((per_cpu_data_base + 40) as *mut usize, rq_ptr);
	}
}

/*
 * per_cpu_data_base - Read the per-CPU data area base address
 *
 * Return: Virtual address of PerCpuData, or 0 if not initialized
 */
pub fn per_cpu_data_base() -> usize {
	unsafe { PER_CPU_DATA_BASE }
}

/*
 * get_per_cpu_run_queue - Get reference to per-CPU run queue
 * @cpu_id: CPU ID
 *
 * Panics if init_per_cpu() has not been called for this CPU.
 *
 * Return: Reference to the per-CPU Mutex<RunQueue>
 */
pub fn get_per_cpu_run_queue(cpu_id: u8) -> &'static Mutex<RunQueue> {
	let once_ref = PER_CPU_RUN_QUEUES
		.get()
		.expect("Per-CPU run queues not initialized — call init_per_cpu() first");
	let once_inner = &once_ref[cpu_id as usize];
	once_inner.get().expect("Per-CPU run queue not initialized — call init_per_cpu() for CPU")
}

/*
 * enqueue_task - Enqueue a task into the current CPU's run queue
 * @task: Arc-wrapped task to enqueue
 *
 * Convenience wrapper around get_per_cpu_run_queue().lock().enqueue().
 */
pub fn enqueue_task(task: Arc<Mutex<TaskCB>>) {
	get_per_cpu_run_queue(super::scheduler::current_cpu_id())
		.lock()
		.enqueue(task);
}

/*
 * wake_task - Wake a blocked task by setting it Ready and enqueuing it
 * @task: Arc-wrapped task to wake
 *
 * Used by subsystems (IPC, timers) to unblock a waiting task.
 * The task's state is set to Ready by enqueue().
 *
 * Safety: Acquires RunQueue lock. Must not be called while RunQueue
 *         lock is already held.
 */
pub fn wake_task(task: Arc<Mutex<TaskCB>>) {
	get_per_cpu_run_queue(super::scheduler::current_cpu_id())
		.lock()
		.enqueue(task);
}

/*
 * current_cpu_id - Read the LAPIC ID of the current CPU via MSR 0x1B
 *
 * Return: CPU ID as u8
 */
pub fn current_cpu_id() -> u8 {
	let mut apic_id: u64;
	unsafe {
		core::arch::asm!("rdmsr", in("ecx") 0x1Bu32, lateout("eax") apic_id, lateout("edx") _);
	}
	(apic_id & 0xFF) as u8
}

/*
 * current_task_id - Get the task ID of the currently running task
 *
 * Return: TaskId value, or 0 if no task is running
 */
pub fn current_task_id() -> u64 {
	CURRENT_TASK.load(Ordering::Acquire)
}

/*
 * current_task_arc - Get Arc reference to the currently running task
 *
 * Return: Some(Arc<Mutex<TaskCB>>) if a task is running, None otherwise
 *
 * Safety: Acquires RunQueue lock briefly. Must not be called while
 *         RunQueue lock is already held.
 */
pub fn current_task_arc() -> Option<Arc<Mutex<TaskCB>>> {
	get_per_cpu_run_queue(current_cpu_id()).lock().current.clone()
}

/*
 * take_current - Remove the current task without re-enqueuing
 *
 * Unlike reschedule_current(), this removes the current task from the
 * RunQueue entirely. The caller is responsible for holding onto the
 * returned Arc (e.g., placing it on a wait queue).
 *
 * Return: Some(Arc<Mutex<TaskCB>>) if a task was running, None otherwise
 *
 * Safety: Must be called with interrupts disabled.
 *         Caller must ensure the task is eventually re-enqueued or destroyed.
 */
pub fn take_current() -> Option<Arc<Mutex<TaskCB>>> {
	get_per_cpu_run_queue(current_cpu_id()).lock().current.take()
}

/*
 * pick_next_task() - Select the next task to run from the per-CPU run queue
 *
 * Dequeues the front task and transitions it to Running state.
 * Updates CURRENT_TASK atomic with the selected task's ID.
 *
 * Return: Some(task) if a runnable task exists, None if queue is empty
 *
 * Called with interrupts disabled (inside timer interrupt handler)
 * Safety: Must not be called concurrently - single-CPU invariant
 */
pub fn pick_next_task() -> Option<Arc<Mutex<TaskCB>>> {
	let cpu_id = current_cpu_id();
	let mut rq = get_per_cpu_run_queue(cpu_id).lock();
	let next = rq.dequeue_scheduled()?;
	{
		let mut task = next.lock();
		task.set_state(TaskState::Running);
		CURRENT_TASK.store(task.id.0, Ordering::Release);
	}
	rq.current = Some(Arc::clone(&next));
	Some(next)
}

/*
 * reschedule_current - Re-enqueue the current task at the back of the queue
 *
 * Moves the running task back to Ready state and places it at the tail
 * of the per-CPU run queue, implementing round-robin fairness.
 *
 * Called before pick_next_task() to yield the current time slice.
 *
 * Safety: Must not be called if no task is currently running.
 *         Must be called with interrupts disabled.
 */
pub fn reschedule_current() {
	let cpu_id = current_cpu_id();
	let mut rq = get_per_cpu_run_queue(cpu_id).lock();
	if let Some(task) = rq.current.take() {
		crate::wfq::update_virtual_runtime(&mut task.lock());
		/* Skip re-enqueue for the boot placeholder (kstack == 0) */
		let dominated = task.lock().kstack.as_u64() == 0;
		if !dominated {
			rq.enqueue(task);
		}
	}
}

/*
 * schedule - Yield current task and switch to the next runnable task
 *
 * This is the main scheduling entry point. It re-enqueues the current
 * task at the back of the per-CPU run queue, then selects the next task.
 * If no other task is available, the current task continues.
 *
 * NOTE: Context switch is NOT performed here - that is wired later.
 *       This function establishes the task selection logic only.
 *
 * Return: Some(next_task) selected for execution, None if queue was empty
 *         before re-enqueue
 *
 * Safety: Must be called with interrupts disabled (timer IRQ handler context)
 */
pub fn schedule() -> Option<Arc<Mutex<TaskCB>>> {
	reschedule_current();
	pick_next_task()
}

/*
 * global_or_none - Get per-CPU RunQueue reference without panicking
 *
 * Return: Some(&Mutex<RunQueue>) if initialized for this CPU, None otherwise
 */
pub fn global_or_none(cpu_id: u8) -> Option<&'static Mutex<RunQueue>> {
	let arr = PER_CPU_RUN_QUEUES.get()?;
	let once_ref = arr.get(cpu_id as usize)?;
	once_ref.get()
}

/*
 * push_zombie - Move a task to the zombie list after exit
 * @task: Arc to the exited task
 */
pub fn push_zombie(task: Arc<Mutex<TaskCB>>) {
	get_per_cpu_run_queue(current_cpu_id()).lock().zombies.push(task);
}

/*
 * find_task_by_id - Look up any live task by numeric ID
 * @id: TaskId value to find
 *
 * Searches the current task and the run queue on the current CPU.
 * Does not search zombies.
 * Return: Some(Arc) if found, None otherwise.
 */
pub fn find_task_by_id(id: u64) -> Option<Arc<Mutex<TaskCB>>> {
	let cpu_id = current_cpu_id();
	let rq = get_per_cpu_run_queue(cpu_id).lock();
	if let Some(ref current) = rq.current {
		if current.lock().id.0 == id {
			return Some(Arc::clone(current));
		}
	}
	for task in rq.queue.iter() {
		if task.lock().id.0 == id {
			return Some(Arc::clone(task));
		}
	}
	None
}

/*
 * find_zombie_child - Find and remove a zombie child of the given parent
 * @parent_id: Task ID of the parent
 * @child_pid: Specific child to wait for (-1 = any child)
 *
 * Return: Some(Arc) of the zombie TaskCB if found and removed, None otherwise.
 */
pub fn find_zombie_child(parent_id: u64, child_pid: i64) -> Option<Arc<Mutex<TaskCB>>> {
	let cpu_id = current_cpu_id();
	let mut rq = get_per_cpu_run_queue(cpu_id).lock();
	let pos = rq
		.zombies
		.iter()
		.position(|z| {
			let task = z.lock();
			task.parent_id == parent_id
				&& (child_pid == -1 || task.id.0 == child_pid as u64)
		})?;
	Some(rq.zombies.remove(pos))
}

/*
 * Lock Registry — tracks which task holds which lock for priority inheritance.
 *
 * Keys are the raw pointer address of the lock (as usize).
 * Values are Arc<Mutex<TaskCB>> of the task currently holding the lock.
 *
 * This is a global registry because locks can be shared across CPUs and tasks.
 * All access must be done with interrupts disabled (timer IRQ context).
 */
static LOCK_REGISTRY: Once<Mutex<BTreeMap<usize, Arc<Mutex<TaskCB>>>>> = Once::new();

/*
 * lock_registry - Get the global lock registry
 *
 * Return: Reference to the lock registry
 */
fn lock_registry() -> &'static Mutex<BTreeMap<usize, Arc<Mutex<TaskCB>>>> {
	LOCK_REGISTRY.call_once(|| Mutex::new(BTreeMap::new()))
}

/*
 * lock_register - Register a lock holder in the registry
 * @lock_ptr: Raw pointer address of the lock
 * @holder: Task that acquired the lock
 *
 * Safety: Caller must ensure lock_ptr is valid and points to a real lock.
 */
pub fn lock_register(lock_ptr: usize, holder: Arc<Mutex<TaskCB>>) {
	let reg = lock_registry();
	reg.lock().insert(lock_ptr, holder);
}

/*
 * lock_unregister - Remove a lock from the registry
 * @lock_ptr: Raw pointer address of the lock
 *
 * Returns the holder task if it was registered, None otherwise.
 */
pub fn lock_unregister(lock_ptr: usize) -> Option<Arc<Mutex<TaskCB>>> {
	lock_registry().lock().remove(&lock_ptr)
}

/*
 * lock_find_holder - Find the task holding a given lock
 * @lock_ptr: Raw pointer address of the lock
 *
 * Return: Some(Arc<Mutex<TaskCB>>) of the holder, None if no one holds it.
 */
pub fn lock_find_holder(lock_ptr: usize) -> Option<Arc<Mutex<TaskCB>>> {
	lock_registry().lock().get(&lock_ptr).cloned()
}

/*
 * acquire_lock_with_pi - Acquire a lock with priority inheritance
 * @lock_ptr: Raw pointer address of the lock being acquired
 * @task: The task acquiring the lock
 *
 * If the lock is already held by a lower-priority task, boost that task's
 * priority to match the acquiring task's priority. This prevents priority
 * inversion where a high-priority task blocks on a low-priority task that
 * holds a shared resource.
 *
 * The caller is responsible for actually acquiring the lock (e.g., via
 * spin::Mutex::lock()). This function only handles the priority boost.
 *
 * After acquiring the lock, call lock_register() to record ownership.
 */
pub fn acquire_lock_with_pi(lock_ptr: usize, task: &Arc<Mutex<TaskCB>>) {
	let task_priority = task.lock().priority();

	/* Check if any task currently holds this lock */
	if let Some(holder) = lock_find_holder(lock_ptr) {
		let holder_priority = holder.lock().priority();

		/* If holder has lower priority (higher number), boost it */
		if holder_priority > task_priority {
			boost_priority(&holder, task_priority);
		}
	}
}

/*
 * release_lock_with_pi - Release a lock and restore holder's original priority
 * @task: The task releasing the lock
 *
 * Restores the holder's original priority if it was boosted by priority inheritance.
 * The caller must call lock_unregister() to remove the lock from the registry.
 */
pub fn release_lock_with_pi(task: &Arc<Mutex<TaskCB>>) {
	let mut task_guard = task.lock();

	/* Restore original priority if it was boosted */
	if let Some(orig_priority) = task_guard.inherited_priority {
		task_guard.sched_class = SchedClass::Fair(orig_priority);
		task_guard.inherited_priority = None;
	}
}

/*
 * boost_priority - Temporarily boost a task's priority for PI
 * @task: Task to boost
 * @new_priority: Priority to boost to (lower number = higher priority)
 *
 * Saves the original priority and sets the new one.
 */
pub fn boost_priority(task: &Arc<Mutex<TaskCB>>, new_priority: u8) {
	let mut task_guard = task.lock();
	if task_guard.inherited_priority.is_none() {
		match task_guard.sched_class {
			SchedClass::Fair(p) => task_guard.inherited_priority = Some(p),
			SchedClass::Realtime(p) => task_guard.inherited_priority = Some(p),
			SchedClass::Batch => task_guard.inherited_priority = Some(140),
			SchedClass::Iso => task_guard.inherited_priority = Some(50),
		}
	}
	task_guard.sched_class = SchedClass::Fair(new_priority);
}

/*
 * restore_priority - Restore a task's original priority
 * @task: Task to restore
 *
 * Restores the task's original priority if it was temporarily boosted.
 */
pub fn restore_priority(task: &Arc<Mutex<TaskCB>>) {
	let mut task_guard = task.lock();
	if let Some(orig_priority) = task_guard.inherited_priority {
		task_guard.sched_class = SchedClass::Fair(orig_priority);
		task_guard.inherited_priority = None;
	}
}

/*
 * inject_direct_message - Inject a message directly into a task's TaskCB
 * @receiver: The task that will receive the message
 * @msg: The message to inject
 *
 * Used by the IPC fastpath to bypass the message queue when the receiver
 * is blocked at receive_blocking(). The message is stored in the task's
 * direct_msg field and flagged as valid.
 *
 * The receiver will see this message on the next loop iteration of
 * receive_blocking() before checking the queue.
 */
pub fn inject_direct_message(receiver: &Arc<Mutex<TaskCB>>, msg: Arc<ipc_types::Message>) {
	let mut task = receiver.lock();
	task.direct_msg = Some((*msg).clone());
	task.direct_msg_valid = true;
}

/*
 * consume_direct_message - Check and consume a direct message from the current task
 *
 * Returns Some(msg) if a direct message is pending, None otherwise.
 * Clears the direct_msg_valid flag after reading.
 *
 * Called by receive_blocking() to check for fastpath messages.
 */
pub fn consume_direct_message() -> Option<ipc_types::Message> {
	let current = current_task_arc()?;
	let mut task = current.lock();
	if task.direct_msg_valid {
		task.direct_msg_valid = false;
		task.direct_msg.take()
	} else {
		None
	}
}
