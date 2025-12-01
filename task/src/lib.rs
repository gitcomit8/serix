/*
 * Task Scheduling and Management
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
use core::arch::naked_asm;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::{Context, Poll};
use spin::Mutex;
use x86_64::registers::model_specific::GsBase;
use x86_64::VirtAddr;

pub static mut STACK_UPDATE_HOOK: Option<fn(VirtAddr)> = None;
pub unsafe fn register_stack_update_hook(func: fn(VirtAddr)) {
	STACK_UPDATE_HOOK = Some(func);
}

pub struct Executor {
	tasks: VecDeque<AsyncTask>,
	current_task_index: usize,
}

impl Executor {
	pub fn new() -> Self {
		Executor {
			tasks: VecDeque::new(),
			current_task_index: 0,
		}
	}

	pub fn spawn(&mut self, task: AsyncTask) {
		self.tasks.push_back(task);
	}

	pub fn poll_all(&mut self) {
		let waker = crate::waker::dummy_waker();
		let mut ctx = Context::from_waker(&waker);
		let len = self.tasks.len();

		for _ in 0..len {
			if let Some(mut task) = self.tasks.pop_front() {
				match task.poll(&mut ctx) {
					Poll::Ready(()) => {
						// Task finished, drop
					}
					Poll::Pending => self.tasks.push_back(task),
				}
			}
		}
	}

	pub fn task_yield(&mut self) {
		if !self.tasks.is_empty() {
			self.current_task_index = (self.current_task_index + 1) % self.tasks.len();
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskId(pub u64);

impl TaskId {
	//Generate unique task id
	pub fn new() -> Self {
		static NEXT_ID: AtomicU64 = AtomicU64::new(1);
		TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
	}

	pub fn as_u64(self) -> u64 {
		self.0
	}
}

//Task states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
	Ready,
	Running,
	Blocked,
	Terminated,
}

//Scheduling class
#[derive(Debug, Copy, Clone)]
pub enum SchedClass {
	Realtime(u8), //0-99 RT FIFO
	Fair(u8),     //100-139 FWS
	Batch,        //140 Batch
	Iso,          //Isochronous
}

impl Default for SchedClass {
	fn default() -> Self {
		SchedClass::Fair(120) //Default normal priority
	}
}

//CPU context for task switching
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CPUContext {
	//Callee-saved registers (SYS-V ABI)
	pub rsp: u64, //Stack pointer
	pub rbp: u64, //Base pointer
	pub rbx: u64,
	pub r12: u64,
	pub r13: u64,
	pub r14: u64,
	pub r15: u64,

	//Execution context
	pub rip: u64,
	pub rflags: u64,

	//Segment selectors
	pub cs: u64,
	pub ss: u64,

	//Optional: FS/GS base MSRs, CR3 for paging
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

//Task Control Block
#[derive(Debug, Clone)]
pub struct TaskCB {
	pub id: TaskId,
	pub state: TaskState,
	pub sched_class: SchedClass,
	pub context: CPUContext,
	pub kstack: VirtAddr,
	pub ustack: Option<VirtAddr>,
	pub name: &'static str,
}

//trampoline function called via context switch
extern "C" fn task_trampoline(entry_point: extern "C" fn() -> !) -> ! {
	entry_point();
	loop {
		x86_64::instructions::hlt();
	}
}

impl TaskCB {
	//Create new kernel task
	pub fn new(
		name: &'static str,
		entry_point: unsafe extern "C" fn() -> !,
		stack: VirtAddr,
		sched_class: SchedClass,
	) -> Self {
		let mut context = CPUContext::default();
		// Align the stack pointer down to 16-byte boundary (required ABI)
		let rsp = stack.as_u64() & !0xF;

		//Setup context registers
		context.rsp = rsp;
		context.rbp = 0;
		context.rip = entry_point as u64; // Jump directly to entry point
		context.rflags = 0x202; //IF=1, reserved bit set
		context.cs = 0x8; //Kernel code segment
		context.ss = 0x10; //Kernel stack segment
		context.ds = 0x10; //Kernel data segment
		context.es = 0x10; //Kernel data segment
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
			sched_class,
			context,
			kstack: stack,
			ustack: None,
			name,
		}
	}

	pub fn running_task() -> Self {
		Self {
			id: TaskId::new(),
			state: TaskState::Running,
			sched_class: SchedClass::default(),
			context: CPUContext::default(),
			kstack: VirtAddr::zero(), // Placeholder
			ustack: None,
			name: "kernel_main",
		}
	}

	//Set the task state
	pub fn set_state(&mut self, state: TaskState) {
		self.state = state;
	}

	//Get task priority
	pub fn priority(&self) -> u8 {
		match self.sched_class {
			SchedClass::Realtime(p) => p,
			SchedClass::Fair(p) => p,
			SchedClass::Batch => 140,
			SchedClass::Iso => 50, //High priority
		}
	}
	/*
	 * clone_thread - Create a new thread sharing the same address space
	 * @entry_point: User Instruction Pointer (RIP) for the new thread
	 * @user_stack: User Stack Pointer (RSP) for the new thread
	 *
	 * Returns a new TaskCB that shares CR3 with self but has a fresh Context.
	 */
	pub fn clone_thread(&self, entry_point: u64, user_stack: u64) -> Self {
		// Allocate separate kernel stack for the thread
		// Note: In a real OS, manage this lifecycle properly (e.g., Slab Allocator)
		let kstack = alloc::vec![0u8; 16384];
		let kstack_bottom = kstack.as_ptr() as u64;
		let kstack_top = kstack_bottom + 16384;

		// 1. Construct Interrupt Stack Frame (Trap Frame) for IRETQ
		// We push the User Context onto the NEW Kernel Stack.
		// Layout (growing down): SS, RSP, RFLAGS, CS, RIP
		let mut sp = kstack_top;
		unsafe {
			// SS (User Data Selector - Ring 3)
			sp -= 8;
			*(sp as *mut u64) = 0x23;

			// RSP (User Stack Pointer)
			sp -= 8;
			*(sp as *mut u64) = user_stack;

			// RFLAGS (Interrupts Enabled, IOPL=3 maybe? Default 0x202 is safe)
			sp -= 8;
			*(sp as *mut u64) = 0x202;

			// CS (User Code Selector - Ring 3)
			sp -= 8;
			*(sp as *mut u64) = 0x2b;

			// RIP (User Entry Point)
			sp -= 8;
			*(sp as *mut u64) = entry_point;
		}

		// 2. Setup Kernel Context for the Scheduler
		// When the scheduler picks this task, it will context_switch INTO this context.
		// It must start in Kernel Mode (Ring 0) and execute the trampoline to jump to User Mode.
		let mut context = CPUContext::default();
		context.rip = enter_user_mode as usize as u64; // Trampoline
		context.rsp = sp; // Stack Pointer points to the IRETQ frame we just built
		context.rflags = 0x2;

		// Standard Kernel Segments for Ring 0 execution
		context.cs = 0x8;
		context.ss = 0x10;
		// Load User Data segments into DS/ES/FS/GS so they are ready after IRET
		context.ds = 0x23;
		context.es = 0x23;
		context.fs = 0x23;
		context.gs = 0x23;

		context.cr3 = self.context.cr3; // Share Address Space

		// 3. Setup GS Base
		// We must capture the current Kernel GS Base (PerCPU Data) so it is restored
		// correctly when the scheduler switches to this task.
		unsafe {
			context.gs_base = GsBase::read().as_u64();
		}

		// Prevent deallocation of the stack vector (leak it for now)
		core::mem::forget(kstack);

		TaskCB {
			id: TaskId::new(),
			state: TaskState::Ready,
			sched_class: self.sched_class,
			context,
			// kstack must point to the absolute top for TSS updates
			kstack: VirtAddr::new(kstack_top),
			ustack: Some(VirtAddr::new(user_stack)),
			name: "user_thread",
		}
	}
}

#[unsafe(naked)]
unsafe extern "C" fn enter_user_mode() {
	naked_asm!("xor rax, rax", "swapgs", "iretq");
}

//Task creation parameters
pub struct TaskBuilder {
	name: &'static str,
	sched_class: SchedClass,
	stack_size: usize,
}

impl TaskBuilder {
	pub fn new(name: &'static str) -> Self {
		Self {
			name,
			sched_class: SchedClass::default(),
			stack_size: 8192,
		}
	}

	pub fn sched_class(mut self, sched_class: SchedClass) -> Self {
		self.sched_class = sched_class;
		self
	}

	pub fn stack_size(mut self, size: usize) -> Self {
		self.stack_size = size;
		self
	}

	//Build a kernel task
	pub fn build_kernel_task(self, entry_point: unsafe extern "C" fn() -> !) -> TaskCB {
		//TODO: Allocate stack memory properly
		let stack_base = VirtAddr::new(0xFFFF_FF80_0000_0000); //Placeholder
		let stack_top = stack_base + self.stack_size as u64;

		TaskCB::new(self.name, entry_point, stack_top, self.sched_class)
	}
}

//Scheduler - performs actual context switching and task management
pub struct Scheduler {
	pub tasks: Vec<TaskCB>,
	pub current: usize,
}

// Global scheduler instance
static GLOBAL_SCHEDULER: spin::Once<spin::Mutex<Scheduler>> = spin::Once::new();

impl Scheduler {
	pub fn new() -> Self {
		Self {
			tasks: Vec::new(),
			current: 0,
		}
	}

	pub fn init_global() {
		GLOBAL_SCHEDULER.call_once(|| spin::Mutex::new(Scheduler::new()));
	}

	pub fn global() -> &'static spin::Mutex<Scheduler> {
		GLOBAL_SCHEDULER.get().expect("Scheduler not initialized")
	}

	// Start the scheduler - must be called without holding the lock
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

		let next_task = &scheduler.tasks[next_idx];
		let new_kstack = next_task.kstack;

		hal::serial_println!("Switching from task {} to task {}", current_idx, next_idx);

		// Get raw pointers
		let old_ctx = &mut scheduler.tasks[current_idx].context as *mut CPUContext;
		let new_ctx = &scheduler.tasks[next_idx].context as *const CPUContext;

		// Drop the lock before context switch
		drop(scheduler);

		// UPDATE KERNEL STACK (TSS)
		if let Some(hook) = STACK_UPDATE_HOOK {
			hook(new_kstack);
		}

		// Context switch - will return when task yields
		context_switch::context_switch(old_ctx, new_ctx);

		// Should never reach here since tasks never yield back to kernel_boot task
		panic!("Returned to kernel boot task");
	}

	pub fn add_task(&mut self, task: TaskCB) {
		self.tasks.push(task);
	}

	pub fn task_count(&self) -> usize {
		self.tasks.len()
	}

	fn next_ready_task(&mut self) -> Option<usize> {
		let count = self.tasks.len();
		for i in 1..=count {
			let idx = (self.current + i) % count;
			if self.tasks[idx].state == TaskState::Ready {
				return Some(idx);
			}
		}
		None
	}

	pub fn pick_next(&mut self) -> Option<usize> {
		// If current task is Running, mark it Ready so it can be picked again
		if self.tasks[self.current].state == TaskState::Running {
			self.tasks[self.current].state = TaskState::Ready;
		}

		if let Some(next_idx) = self.next_ready_task() {
			self.tasks[next_idx].state = TaskState::Running;
			Some(next_idx)
		} else {
			None
		}
	}
}

// Public API for tasks (and timer) to yield CPU
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
				let new_kstack = scheduler.tasks[next_idx].kstack;

				// Get raw pointers
				let old_ctx = &mut scheduler.tasks[current_idx].context as *mut CPUContext;
				let new_ctx = &scheduler.tasks[next_idx].context as *const CPUContext;

				if let Some(hook) = STACK_UPDATE_HOOK {
					hook(new_kstack);
				}
				(old_ctx, new_ctx)
			} else {
				return; // No ready tasks
			}
		}; // Lock dropped here

		// Perform context switch
		context_switch::context_switch(old_ctx, new_ctx);
	}
}

// Public API for tasks to yield CPU
pub fn task_yield() {
	schedule();
}

pub static EXECUTOR: Mutex<Option<Executor>> = Mutex::new(None);

pub fn init_executor() {
	EXECUTOR.lock().replace(Executor::new());
}

pub fn spawn_task<F>(future: F)
where
	F: Future<Output = ()> + Send + 'static,
{
	use crate::async_task::AsyncTask;
	let mut guard = EXECUTOR.lock();
	if let Some(executor) = guard.as_mut() {
		executor.spawn(AsyncTask::new(future));
	}
}

pub fn poll_executor() {
	let mut guard = EXECUTOR.lock();
	if let Some(executor) = guard.as_mut() {
		executor.poll_all();
	}
}

pub fn preempt_executor() {
	let mut guard = EXECUTOR.lock();
	if let Some(executor) = guard.as_mut() {
		executor.task_yield();
	}
}
