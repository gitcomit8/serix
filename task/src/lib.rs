/*
 * lib.rs - Task Scheduling and Management
 *
 * Implements cooperative multitasking with async/await support.
 * Provides task control blocks, scheduling, context switching, and an async executor.
 */

#![no_std]

extern crate alloc;
pub mod async_task;
pub mod context_switch;
pub mod executor;
pub mod waker;
pub mod yield_now;

use crate::async_task::AsyncTask;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::{Context, Poll};
use spin::Mutex;
use x86_64::VirtAddr;

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
	 *
	 * Return: New Executor instance
	 */
	pub fn new() -> Self {
		Executor {
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
	 * poll_all - Poll all tasks once
	 *
	 * Makes one pass through all pending tasks, removing completed ones.
	 */
	pub fn poll_all(&mut self) {
		let waker = crate::waker::dummy_waker();
		let mut ctx = Context::from_waker(&waker);
		let len = self.tasks.len();

		for _ in 0..len {
			if let Some(mut task) = self.tasks.pop_front() {
				match task.poll(&mut ctx) {
					Poll::Ready(()) => {
						// Task finished, drop it
					}
					Poll::Pending => self.tasks.push_back(task),
				}
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

/*
 * struct TaskId - Unique task identifier
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskId(pub u64);

impl TaskId {
	/*
	 * new - Generate unique task ID
	 *
	 * Return: New unique TaskId
	 */
	pub fn new() -> Self {
		static NEXT_ID: AtomicU64 = AtomicU64::new(1);
		TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
	}

	/*
	 * as_u64 - Get task ID as u64
	 *
	 * Return: Numeric task ID
	 */
	pub fn as_u64(self) -> u64 {
		self.0
	}
}

/*
 * enum TaskState - Task states
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
	Ready,
	Running,
	Blocked,
	Terminated,
	Dead,
}

/*
 * struct CPUContext - CPU context for task switching
 * @rsp: Stack pointer
 * @rbp: Base pointer
 * @rbx: Callee-saved register
 * @r12: Callee-saved register
 * @r13: Callee-saved register
 * @r14: Callee-saved register
 * @r15: Callee-saved register
 * @rip: Instruction pointer
 * @rflags: CPU flags register
 * @cs: Code segment selector
 * @ss: Stack segment selector
 * @fs: FS segment selector
 * @gs: GS segment selector
 * @ds: DS segment selector
 * @es: ES segment selector
 * @fs_base: FS base MSR value
 * @gs_base: GS base MSR value
 * @cr3: Page table base address
 */
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CPUContext {
	// Callee-saved registers (SYS-V ABI)
	pub rsp: u64,
	pub rbp: u64,
	pub rbx: u64,
	pub r12: u64,
	pub r13: u64,
	pub r14: u64,
	pub r15: u64,

	// Execution context
	pub rip: u64,
	pub rflags: u64,

	// Segment selectors
	pub cs: u64,
	pub ss: u64,

	// Optional: FS/GS base MSRs, CR3 for paging
	pub fs: u64,
	pub gs: u64,
	pub ds: u64,
	pub es: u64,
	pub fs_base: u64,
	pub gs_base: u64,
	pub cr3: u64,
}

impl Default for CPUContext {
	fn default() -> Self {
		Self {
			rsp: 0,
			rbp: 0,
			rbx: 0,
			r12: 0,
			r13: 0,
			r14: 0,
			r15: 0,
			rip: 0,
			rflags: 0x202, // Interrupt flag set and Reserved bit
			cs: 0x8,       // Typical kernel code segment selector
			fs: 0,
			gs: 0,
			ss: 0x10, // Typical kernel stack segment selector
			ds: 0,
			es: 0,
			fs_base: 0,
			gs_base: 0,
			cr3: 0,
		}
	}
}

/*
 * struct TaskCB - Task Control Block
 * @id: Unique task identifier
 * @state: Current task state
 * @priority: Scheduling priority (0 = highest, 255 = lowest)
 * @context: Saved CPU context
 * @kstack: Kernel stack pointer
 * @ustack: Optional user stack pointer
 * @name: Task name (static string)
 */
#[derive(Debug, Clone)]
pub struct TaskCB {
	pub id: TaskId,
	pub state: TaskState,
	pub priority: u8,
	pub context: CPUContext,
	pub kstack: VirtAddr,
	pub ustack: Option<VirtAddr>,
	pub name: &'static str,
}

/*
 * task_trampoline - Trampoline function called via context switch
 * @entry_point: Function to execute
 *
 * Wrapper that calls the task entry point and halts on return.
 */
extern "C" fn task_trampoline(entry_point: extern "C" fn() -> !) -> ! {
	entry_point();
	loop {
		x86_64::instructions::hlt();
	}
}

impl TaskCB {
	/*
	 * new - Create new kernel task
	 * @name: Task name
	 * @entry_point: Entry point function
	 * @stack: Stack top address
	 * @priority: Scheduling priority (0 = highest, 255 = lowest)
	 *
	 * Return: New TaskCB instance
	 */
	pub fn new(
		name: &'static str,
		entry_point: unsafe extern "C" fn() -> !,
		stack: VirtAddr,
		priority: u8,
	) -> Self {
		let mut context = CPUContext::default();
		// Align the stack pointer down to 16-byte boundary (required ABI)
		let rsp = stack.as_u64() & !0xF;

		// Setup context registers
		context.rsp = rsp;
		context.rbp = 0;
		context.rip = entry_point as u64; // Jump directly to entry point
		context.rflags = 0x202; // IF=1, reserved bit set
		context.cs = 0x8; // Kernel code segment
		context.ss = 0x10; // Kernel stack segment
		context.ds = 0x10; // Kernel data segment
		context.es = 0x10; // Kernel data segment
		context.fs = 0;
		context.gs = 0;

		// Get current CR3 value (page table)
		unsafe {
			use x86_64::registers::control::Cr3;
			let (frame, _flags) = Cr3::read();
			context.cr3 = frame.start_address().as_u64();
		}

		Self {
			id: TaskId::new(),
			state: TaskState::Ready,
			priority,
			context,
			kstack: stack,
			ustack: None,
			name,
		}
	}

	/*
	 * running_task - Create a placeholder for the currently running task
	 *
	 * Return: TaskCB for the current kernel main task
	 */
	pub fn running_task() -> Self {
		Self {
			id: TaskId::new(),
			state: TaskState::Running,
			priority: 128, // Default normal priority
			context: CPUContext::default(),
			kstack: VirtAddr::zero(),
			ustack: None,
			name: "kernel_main",
		}
	}

	/*
	 * set_state - Set the task state
	 * @state: New state
	 */
	pub fn set_state(&mut self, state: TaskState) {
		self.state = state;
	}

	/*
	 * priority - Get task priority
	 *
	 * Return: Numeric priority (lower is higher priority)
	 */
	pub fn priority(&self) -> u8 {
		self.priority
	}
}

/*
 * struct TaskBuilder - Task creation parameters
 * @name: Task name
 * @priority: Scheduling priority (0 = highest, 255 = lowest)
 * @stack_size: Stack size in bytes
 */
pub struct TaskBuilder {
	name: &'static str,
	priority: u8,
	stack_size: usize,
}

impl TaskBuilder {
	/*
	 * new - Create a new task builder
	 * @name: Task name
	 *
	 * Return: New TaskBuilder with default parameters
	 */
	pub fn new(name: &'static str) -> Self {
		Self {
			name,
			priority: 128, // Default normal priority
			stack_size: 8192,
		}
	}

	/*
	 * priority - Set task priority
	 * @priority: Scheduling priority (0 = highest, 255 = lowest)
	 *
	 * Return: Self for method chaining
	 */
	pub fn priority(mut self, priority: u8) -> Self {
		self.priority = priority;
		self
	}

	/*
	 * stack_size - Set stack size
	 * @size: Stack size in bytes
	 *
	 * Return: Self for method chaining
	 */
	pub fn stack_size(mut self, size: usize) -> Self {
		self.stack_size = size;
		self
	}

	/*
	 * build_kernel_task - Build a kernel task
	 * @entry_point: Task entry point
	 *
	 * Allocates a unique heap stack for the task so each task has its own
	 * distinct stack region instead of sharing a hardcoded address.
	 * The stack is leaked (forgotten) so it lives for the kernel's lifetime.
	 *
	 * Return: TaskCB for the new task
	 */
	pub fn build_kernel_task(self, entry_point: unsafe extern "C" fn() -> !) -> TaskCB {
		let mut stack: Vec<u8> = Vec::with_capacity(self.stack_size);
		stack.resize(self.stack_size, 0);
		let stack_top = VirtAddr::new(stack.as_ptr() as u64) + self.stack_size as u64;
		core::mem::forget(stack); // kernel stack must live forever

		TaskCB::new(self.name, entry_point, stack_top, self.priority)
	}
}

/*
 * struct Scheduler - Performs actual context switching and task management
 * @tasks: List of all tasks
 * @current: Index of currently running task
 */
pub struct Scheduler {
	tasks: Vec<TaskCB>,
	current: usize,
}

// Global scheduler instance
static GLOBAL_SCHEDULER: spin::Once<spin::Mutex<Scheduler>> = spin::Once::new();

impl Scheduler {
	/*
	 * new - Create a new scheduler
	 *
	 * Return: New Scheduler instance
	 */
	pub fn new() -> Self {
		Self {
			tasks: Vec::new(),
			current: 0,
		}
	}

	/*
	 * init_global - Initialize the global scheduler
	 */
	pub fn init_global() {
		GLOBAL_SCHEDULER.call_once(|| spin::Mutex::new(Scheduler::new()));
	}

	/*
	 * global - Get reference to global scheduler
	 *
	 * Return: Reference to global scheduler
	 */
	pub fn global() -> &'static spin::Mutex<Scheduler> {
		GLOBAL_SCHEDULER.get().expect("Scheduler not initialized")
	}

	/*
	 * start - Start the scheduler
	 *
	 * Must be called without holding the lock. Does not return.
	 */
	pub unsafe fn start() -> ! {
		hal::serial_println!("Scheduler::start() called");
		let mut scheduler = Self::global().lock();

		if scheduler.tasks.is_empty() {
			panic!("No tasks available to schedule");
		}

		hal::serial_println!("Scheduler starting, current index: {}", scheduler.current);

		// Do the first context switch to start task execution
		let current_idx = scheduler.current;
		let next_idx = (current_idx + 1) % scheduler.tasks.len();

		scheduler.tasks[current_idx].state = TaskState::Ready;
		scheduler.tasks[next_idx].state = TaskState::Running;
		scheduler.current = next_idx;

		hal::serial_println!("Switching from task {} to task {}", current_idx, next_idx);

		// Get raw pointers
		let old_ctx = &mut scheduler.tasks[current_idx].context as *mut CPUContext;
		let new_ctx = &scheduler.tasks[next_idx].context as *const CPUContext;

		// Drop the lock before context switch
		drop(scheduler);

		// Context switch - will return when task yields
		context_switch::context_switch(old_ctx, new_ctx);

		// Should never reach here since tasks never yield back to kernel_boot task
		panic!("Returned to kernel boot task");
	}

	/*
	 * add_task - Add a task to the scheduler
	 * @task: Task to add
	 */
	pub fn add_task(&mut self, task: TaskCB) {
		self.tasks.push(task);
	}

	/*
	 * task_count - Get number of tasks
	 *
	 * Return: Number of tasks in scheduler
	 */
	pub fn task_count(&self) -> usize {
		self.tasks.len()
	}

	/*
	 * pick_next - Pick next task to run
	 *
	 * Selects the highest-priority (lowest numeric value) ready task.
	 * Returns the index of the selected task, or None if none available.
	 */
	pub fn pick_next(&mut self) -> Option<usize> {
		// If current task is Running, mark it Ready so it can be picked again
		if self.tasks[self.current].state == TaskState::Running {
			self.tasks[self.current].state = TaskState::Ready;
		}

		// Find the highest-priority (lowest priority value) ready task
		let mut best_idx: Option<usize> = None;
		let mut best_priority = u8::MAX;
		for (i, task) in self.tasks.iter().enumerate() {
			if task.state == TaskState::Ready && task.priority <= best_priority {
				best_priority = task.priority;
				best_idx = Some(i);
			}
		}

		if let Some(next_idx) = best_idx {
			self.tasks[next_idx].state = TaskState::Running;
			Some(next_idx)
		} else {
			None
		}
	}
}

/*
 * schedule - Public API for tasks (and timer) to yield CPU
 *
 * Performs context switch to the next ready task.
 */
pub fn schedule() {
	unsafe {
		let (old_ctx, new_ctx) = {
			let mut scheduler = Scheduler::global().lock();
			let current_idx = scheduler.current;

			if let Some(next_idx) = scheduler.pick_next() {
				// Don't switch if it's the same task
				if current_idx == next_idx {
					return;
				}

				scheduler.current = next_idx;

				// Get raw pointers
				let old_ctx = &mut scheduler.tasks[current_idx].context as *mut CPUContext;
				let new_ctx = &scheduler.tasks[next_idx].context as *const CPUContext;

				(old_ctx, new_ctx)
			} else {
				return; // No ready tasks
			}
		}; // Lock dropped here

		// Perform context switch
		context_switch::context_switch(old_ctx, new_ctx);
	}
}

/*
 * task_yield - Public API for tasks to yield CPU
 */
pub fn task_yield() {
	schedule();
}

/*
 * exit_current_task - Terminate the current task and yield to the scheduler
 *
 * Marks the current task as Dead so it will never be scheduled again,
 * then calls schedule() to switch to the next ready task. Does not return.
 */
pub fn exit_current_task() -> ! {
	{
		let mut scheduler = Scheduler::global().lock();
		let current = scheduler.current;
		scheduler.tasks[current].state = TaskState::Dead;
	}
	schedule();
	unreachable!("exit_current_task must not return");
}

/*
 * current_task_id - Get the ID of the currently running task
 *
 * Return: Task ID of the current task as u64
 */
pub fn current_task_id() -> u64 {
	let scheduler = Scheduler::global().lock();
	scheduler.tasks[scheduler.current].id.0
}

// Global executor instance
pub static EXECUTOR: Mutex<Option<Executor>> = Mutex::new(None);

/*
 * init_executor - Initialize the global executor
 */
pub fn init_executor() {
	EXECUTOR.lock().replace(Executor::new());
}

/*
 * spawn_task - Spawn a new async task
 * @future: Future to execute
 */
pub fn spawn_task<F>(future: F)
where
	F: core::future::Future<Output = ()> + Send + 'static,
{
	use crate::async_task::AsyncTask;
	let mut guard = EXECUTOR.lock();
	if let Some(executor) = guard.as_mut() {
		executor.spawn(AsyncTask::new(future));
	}
}

/*
 * poll_executor - Poll all tasks in the executor
 */
pub fn poll_executor() {
	let mut guard = EXECUTOR.lock();
	if let Some(executor) = guard.as_mut() {
		executor.poll_all();
	}
}

/*
 * preempt_executor - Preempt current task
 */
pub fn preempt_executor() {
	let mut guard = EXECUTOR.lock();
	if let Some(executor) = guard.as_mut() {
		executor.task_yield();
	}
}
