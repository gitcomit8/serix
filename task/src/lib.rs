#![no_std]

use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::VirtAddr;

//Task Identifier
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
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
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum TaskState{
	Ready,
	Running,
	Blocked,
	Terminated,
}

//Scheduling class
#[derive(Debug,Copy,Clone)]
pub enum SchedClass{
	Realtime(u8), //0-99 RT FIFO
	Fair(u8),     //100-139 FWS
	Batch,		  //140 Batch
	Iso,		  //Isochronous
}

impl Default for SchedClass{
	fn default() -> Self {
		SchedClass::Fair(120) //Default normal priority
	}
}

//CPU context for task switching
#[repr(C)]
#[derive(Debug,Clone)]
pub struct CPUContext{
	//Callee-saved registers (SYS-V ABI)
	pub rsp: u64,	//Stack pointer
	pub rbp: u64,	//Base pointer
	pub rbx: u64,
	pub r12: u64,
	pub r13: u64,
	pub r14: u64,
	pub r15: u64,
	pub rip: u64,
	pub rflags: u64,
	pub cs: u64,
	pub fs: u64,
	pub gs: u64,
	pub ss: u64,
	pub ds: u64,
	pub es: u64,
	pub fs_base: u64,
	pub gs_base: u64,
	pub cr3: u64,
}

impl Default for CPUContext{
	fn default() -> Self {
		Self{
			rsp: 0,
			rbp: 0,
			rbx: 0,
			r12: 0,
			r13: 0,
			r14: 0,
			r15: 0,
			rip: 0,
			rflags: 0x200, // Default IF=1
			cs: 0x8,        // Typical kernel code segment selector
			fs: 0,
			gs: 0,
			ss: 0x10,       // Typical kernel stack segment selector
			ds: 0,
			es: 0,
			fs_base: 0,
			gs_base: 0,
			cr3: 0,
		}
	}
}

//Task Control Block
#[derive(Debug)]
pub struct TaskCB{
	pub id: TaskId,
	pub state: TaskState,
	pub sched_class: SchedClass,
	pub context: CPUContext,
	pub kstack: VirtAddr,
	pub ustack: Option<VirtAddr>,
	pub name: &'static str,
}

impl TaskCB{
	//Create new kernel task
	pub fn new(name: &'static str, entry_point: VirtAddr, stack: VirtAddr, sched_class: SchedClass) -> Self{
		let mut context=CPUContext::default();
		context.rip=entry_point.as_u64();
		context.rsp=stack.as_u64();
		TaskCB{
			id: TaskId::new(),
			state: TaskState::Ready,
			sched_class,
			context,
			kstack: stack,
			ustack: None,
			name,
		}
	}

	//Set the task state
	pub fn set_state(&mut self, state: TaskState){
		self.state=state;
	}

	//Get task priority
	pub fn priority(&self) -> u8{
	match self.sched_class{
			SchedClass::Realtime(p) => p,
			SchedClass::Fair(p) => p,
			SchedClass::Batch => 140,
			SchedClass::Iso => 50, //High priority
		}
	}
}

//Task creation parameters
pub struct TaskBuilder{
	name: &'static str,
	sched_class: SchedClass,
	stack_size: usize,
}

impl TaskBuilder{
	pub fn new(name: &'static str)-> Self{
		Self{
			name,
			sched_class: SchedClass::default(),
			stack_size: 8192,
		}
	}

	pub fn sched_class(mut self, sched_class: SchedClass)-> Self{
		self.sched_class=sched_class;
		self
	}

	pub fn stack_size(mut self, size: usize)-> Self{
		self.stack_size=size;
		self
	}

	//Build a kernel task
	pub fn build_kernel_task(self, entry_point: VirtAddr)-> TaskCB{
		//TODO: Allocate stack memory properly
		let stack_base=VirtAddr::new(0xFFFF_FF80_0000_0000); //Placeholder

		TaskCB::new(self.name,
		entry_point,
		stack_base+self.stack_size as u64,
		self.sched_class
		)
	}
}

//Async task creation proto
pub trait AsyncTask{
	type Output;

	//Poll task for completion
	fn poll(&mut self)-> TaskPoll<Self::Output>;
}

//Task poll result
pub enum TaskPoll<T>{
	Ready(T),
	Pending,
}

//Example async task
pub struct AsyncTaskExample{
	counter: u64,
	target: u64,
}

impl AsyncTaskExample{
	pub fn new(target: u64)-> Self{
		Self{ counter: 0,target}
	}
}

impl AsyncTask for AsyncTaskExample{
	type Output=u64;

	fn poll(&mut self)-> TaskPoll<Self::Output>{
		self.counter+=1;
		if self.counter>=self.target{
			TaskPoll::Ready(self.counter)
		}else{
			TaskPoll::Pending
		}
	}
}

//Global task management interface
pub struct TaskManager{
	next_id: AtomicU64,
}

impl TaskManager{
	pub const fn new()-> Self{
		Self{
			next_id: AtomicU64::new(1),
		}
	}

	//Create a new task using builder
	pub fn create_task(name: &'static str) -> TaskBuilder{
		TaskBuilder::new(name)
	}

	//Spawn async task (proto)
	pub fn spawn_async<T: AsyncTask>(&self, task: T)-> TaskId{
		//TODO: integrate with scheduler
		TaskId::new()
	}
}
