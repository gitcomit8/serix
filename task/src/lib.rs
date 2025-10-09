#![no_std]

extern crate alloc;
pub mod context_switch;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use x86_64::VirtAddr;

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
    pub rsp: u64,    //Stack pointer
    pub rbp: u64,    //Base pointer
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
    pub fn new(name: &'static str, entry_point: unsafe extern "C" fn() -> !, stack: VirtAddr, sched_class: SchedClass) -> Self {
        let mut context = CPUContext::default();
        // Align the stack pointer down to 16-byte boundary (required ABI)
        let rsp = stack.as_u64() & !0xF;

        //Setup context registers
        context.rsp = rsp;
        context.rbp = 0;
        context.rip = entry_point as u64;  // Jump directly to entry point
        context.rflags = 0x202;  //IF=1, reserved bit set
        context.cs = 0x8;        //Kernel code segment
        context.ss = 0x10;       //Kernel stack segment
        context.ds = 0x10;       //Kernel data segment
        context.es = 0x10;       //Kernel data segment
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

        TaskCB::new(self.name,
                    entry_point,
                    stack_top,
                    self.sched_class,
        )
    }
}

//Async task creation proto
pub trait AsyncTask {
    type Output;

    //Poll task for completion
    fn poll(&mut self) -> TaskPoll<Self::Output>;
}

//Task poll result
pub enum TaskPoll<T> {
    Ready(T),
    Pending,
}

//Example async task
pub struct AsyncTaskExample {
    counter: u64,
    target: u64,
}

impl AsyncTaskExample {
    pub fn new(target: u64) -> Self {
        Self { counter: 0, target }
    }
}

impl AsyncTask for AsyncTaskExample {
    type Output = u64;

    fn poll(&mut self) -> TaskPoll<Self::Output> {
        self.counter += 1;
        if self.counter >= self.target {
            TaskPoll::Ready(self.counter)
        } else {
            TaskPoll::Pending
        }
    }
}

//Task Manager - holds tasks in thread-safe manner
pub struct TaskManager {
    tasks: Mutex<RefCell<Vec<TaskCB>>>,
    current_task_idx: Mutex<usize>,
}

impl TaskManager {
    pub const fn new() -> Self {
        Self {
            tasks: Mutex::new(RefCell::new(Vec::new())),
            current_task_idx: Mutex::new(0),
        }
    }

    //Create a new task using builder
    pub fn create_task(name: &'static str) -> TaskBuilder {
        TaskBuilder::new(name)
    }

    //Spawn async task (proto)
    pub fn spawn_async<T: AsyncTask>(&self, task: T) -> TaskId {
        //TODO: integrate with scheduler
        TaskId::new()
    }

    //Add task to task list
    pub fn add_task(&self, task: TaskCB) {
        let mut tasks = self.tasks.lock();
        tasks.borrow_mut().push(task);
    }

    //Pick the next ready task in round-robin
    pub fn next_ready_task(&self) -> Option<TaskCB> {
        let mut tasks = self.tasks.lock();
        let mut idx = *self.current_task_idx.lock();

        if tasks.borrow().is_empty() {
            return None;
        }

        let tasks_ref = tasks.borrow();
        let total_tasks = tasks_ref.len();

        for _ in 0..total_tasks {
            let task = &tasks_ref[idx];
            if task.state == TaskState::Ready {
                *self.current_task_idx.lock() = (idx + 1) % total_tasks;
                return Some(task.clone());
            }
            idx = (idx + 1) % total_tasks;
        }
        None
    }

    //Update task within task list
    pub fn update_task(&self, updated_task: TaskCB) {
        let mut tasks = self.tasks.lock();
        let mut tasks_ref = tasks.borrow_mut();

        for task in tasks_ref.iter_mut() {
            if task.id == updated_task.id {
                *task = updated_task;
                break;
            }
        }
    }

    //Simple scheduler: selects next ready task and marks it running
    pub fn schedule(&self) -> Option<TaskCB> {
        let next_task_opt = self.next_ready_task();

        if let Some(mut task) = next_task_opt {
            task.set_state(TaskState::Running);
            self.update_task(task.clone());
            Some(task)
        } else {
            None
        }
    }
}

//Scheduler - performs actual context switching and task management
pub struct Scheduler {
    tasks: Vec<TaskCB>,
    current: usize,
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
        GLOBAL_SCHEDULER.call_once(|| {
            spin::Mutex::new(Scheduler::new())
        });
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

    pub fn add_task(&mut self, task: TaskCB) {
        self.tasks.push(task);
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

// Public API for tasks to yield CPU
pub fn task_yield() {
    unsafe {
        // Acquire lock, get info, then drop lock before context switch
        let (old_ctx, new_ctx) = {
            let mut scheduler = Scheduler::global().lock();
            let current_idx = scheduler.current;
            
            // Find the next ready task
            let mut next_idx = (current_idx + 1) % scheduler.tasks.len();
            
            while scheduler.tasks[next_idx].state != TaskState::Ready {
                next_idx = (next_idx + 1) % scheduler.tasks.len();
                
                // If we've checked all tasks and none are ready, just return
                if next_idx == current_idx {
                    return;
                }
            }
            
            hal::serial_println!("task_yield: switching from task {} to task {}", current_idx, next_idx);
            
            // Update states
            scheduler.tasks[current_idx].state = TaskState::Ready;
            scheduler.tasks[next_idx].state = TaskState::Running;
            scheduler.current = next_idx;
            
            // Get raw pointers before dropping lock
            let old_ctx = &mut scheduler.tasks[current_idx].context as *mut CPUContext;
            let new_ctx = &scheduler.tasks[next_idx].context as *const CPUContext;
            
            (old_ctx, new_ctx)
        }; // Lock is dropped here
        
        // Perform context switch without holding lock
        context_switch::context_switch(old_ctx, new_ctx);
    }
}
