# Task Management Module

## Overview

The task module provides multitasking infrastructure for the Serix kernel, including task representation, CPU context management, scheduling algorithms, and context switching. This module is foundational for implementing preemptive multitasking, allowing multiple programs to run concurrently on the CPU.

## Architecture

### Components

1. **Task Control Block (TCB)**: Represents a runnable task with its state and context
2. **CPU Context**: Stores processor registers for context switching
3. **Task Manager**: Global task registry and simple scheduler
4. **Scheduler**: Advanced task scheduler with scheduling policies
5. **Context Switch**: Low-level assembly routine for switching between tasks

### Scheduling Classes

Serix supports multiple scheduling classes inspired by Linux CFS (Completely Fair Scheduler):

1. **Realtime** (priorities 0-99): FIFO/RR scheduling for time-critical tasks
2. **Fair** (priorities 100-139): CFS-style scheduling for normal tasks
3. **Batch** (priority 140): Background batch processing
4. **Iso** (Isochronous): Reserved for multimedia/real-time audio/video

## Module Structure

```
task/
├── src/
│   ├── lib.rs              # Task structures, scheduler, task manager
│   └── context_switch.rs   # Low-level context switching (naked asm)
└── Cargo.toml
```

## Task Representation

### Task ID

```rust
pub struct TaskId(pub u64);

impl TaskId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}
```

**Purpose**: Unique identifier for each task.

**Generation**: Atomically incremented counter starting at 1 (0 reserved for "no task").

**Properties**:
- Never reused (64-bit space: 18 quintillion IDs)
- Thread-safe generation
- Cheap to copy (`Copy` trait)

### Task State

```rust
pub enum TaskState {
    Ready,       // Ready to run, waiting for CPU
    Running,     // Currently executing on CPU
    Blocked,     // Waiting for I/O or event
    Terminated,  // Task finished, resources can be freed
}
```

**State Transitions**:
```
       New
        ↓
      Ready ←────┐
        ↓        │
     Running ────┘
      ↓   ↓
Blocked Terminated
      ↓
    Ready
```

**Rules**:
- Only `Ready` tasks can transition to `Running`
- `Running` tasks can transition to `Ready` (preemption), `Blocked` (I/O wait), or `Terminated` (exit)
- `Blocked` tasks transition to `Ready` when event occurs

### Scheduling Class

```rust
pub enum SchedClass {
    Realtime(u8),  // Priority 0-99
    Fair(u8),      // Priority 100-139
    Batch,         // Priority 140
    Iso,           // Isochronous (special priority ~50)
}

impl Default for SchedClass {
    fn default() -> Self {
        SchedClass::Fair(120)  // Default: middle of fair range
    }
}
```

**Realtime (0-99)**:
- Highest priority
- FIFO or round-robin scheduling
- Preempts all lower-priority tasks
- Use cases: Interrupt handlers, device drivers, real-time control

**Fair (100-139)**:
- Normal user tasks
- Time-sliced, proportional CPU sharing
- Priority affects time slice duration
- Use cases: General applications, shells, servers

**Batch (140)**:
- Lowest priority
- Runs only when no other tasks ready
- Large time slices for throughput
- Use cases: Background jobs, maintenance tasks

**Iso (Isochronous)**:
- Special class for multimedia
- Guaranteed minimum CPU time
- Low latency, predictable scheduling
- Use cases: Audio/video players, games

### CPU Context

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CPUContext {
    // Callee-saved registers (SysV ABI)
    pub rsp: u64,    // Stack pointer
    pub rbp: u64,    // Base pointer
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    
    // Execution context
    pub rip: u64,    // Instruction pointer
    pub rflags: u64, // CPU flags
    
    // Segment selectors
    pub cs: u64,     // Code segment
    pub ss: u64,     // Stack segment
    
    // Additional segments
    pub fs: u64,
    pub gs: u64,
    pub ds: u64,
    pub es: u64,
    
    // Base addresses for FS/GS
    pub fs_base: u64,
    pub gs_base: u64,
    
    // Page table base
    pub cr3: u64,
}
```

**Why These Registers?**

**Callee-Saved (must be preserved)**:
- RSP, RBP: Stack management
- RBX, R12-R15: General-purpose registers preserved across function calls

**Execution State**:
- RIP: Next instruction to execute
- RFLAGS: CPU flags (interrupt enable, direction, carry, zero, etc.)

**Segments**:
- CS, SS: Required for ring transitions (kernel ↔ user)
- DS, ES, FS, GS: Data segments
- FS_BASE, GS_BASE: Thread-local storage, per-CPU data

**Memory Management**:
- CR3: Page table base (each task can have its own address space)

**Caller-Saved Registers (not stored)**:
- RAX, RCX, RDX, RSI, RDI, R8-R11: Function arguments and return values
- Saved on stack by calling convention

### Task Control Block

```rust
pub struct TaskCB {
    pub id: TaskId,
    pub state: TaskState,
    pub sched_class: SchedClass,
    pub context: CPUContext,
    pub kstack: VirtAddr,         // Kernel stack
    pub ustack: Option<VirtAddr>, // User stack (if user-mode task)
    pub name: &'static str,
}
```

**Fields**:
- **id**: Unique task identifier
- **state**: Current execution state
- **sched_class**: Scheduling policy and priority
- **context**: CPU register state
- **kstack**: Kernel-mode stack pointer
- **ustack**: User-mode stack pointer (None for kernel tasks)
- **name**: Human-readable name for debugging

## Task Creation

### Task Builder Pattern

```rust
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
            stack_size: 8192,  // 8 KB default
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
    
    pub fn build_kernel_task(self, entry_point: unsafe extern "C" fn() -> !) -> TaskCB {
        // Allocate stack (TODO: proper allocation)
        let stack_base = VirtAddr::new(0xFFFF_FF80_0000_0000);
        let stack_top = stack_base + self.stack_size as u64;
        
        TaskCB::new(self.name, entry_point, stack_top, self.sched_class)
    }
}
```

**Usage Example**:
```rust
let task = TaskBuilder::new("idle_task")
    .sched_class(SchedClass::Fair(120))
    .stack_size(8192)
    .build_kernel_task(idle_task_entry);

Scheduler::global().lock().add_task(task);
```

### Task Initialization

```rust
impl TaskCB {
    pub fn new(
        name: &'static str,
        entry_point: unsafe extern "C" fn() -> !,
        stack: VirtAddr,
        sched_class: SchedClass
    ) -> Self {
        let mut context = CPUContext::default();
        
        // Align stack to 16-byte boundary (SysV ABI requirement)
        let rsp = stack.as_u64() & !0xF;
        
        // Setup initial context
        context.rsp = rsp;
        context.rbp = 0;  // No previous frame
        context.rip = entry_point as u64;  // Start at entry point
        context.rflags = 0x202;  // IF=1 (interrupts enabled), bit 1 always 1
        context.cs = 0x8;   // Kernel code segment
        context.ss = 0x10;  // Kernel stack segment
        context.ds = 0x10;
        context.es = 0x10;
        context.fs = 0;
        context.gs = 0;
        
        // Get current CR3 (page table)
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
}
```

**Key Initialization Steps**:

1. **Stack Alignment**: SysV ABI requires 16-byte aligned stack
2. **RFLAGS**: Set IF=1 (interrupts enabled) and reserved bit 1
3. **Segment Selectors**: Point to kernel code/data segments
4. **CR3**: Copy current page table (tasks share kernel address space initially)
5. **Entry Point**: Set RIP to task's entry function

## Scheduling

### Task Manager (Simple Scheduler)

```rust
pub struct TaskManager {
    tasks: Mutex<RefCell<Vec<TaskCB>>>,
    current_task_idx: Mutex<usize>,
}
```

**Purpose**: Simple round-robin task manager.

**Methods**:

#### Add Task

```rust
pub fn add_task(&self, task: TaskCB)
```

Adds a task to the task list.

#### Next Ready Task

```rust
pub fn next_ready_task(&self) -> Option<TaskCB>
```

**Algorithm**: Round-robin search for next `Ready` task.

```rust
let mut idx = current_task_idx;
for _ in 0..total_tasks {
    let task = &tasks[idx];
    if task.state == TaskState::Ready {
        current_task_idx = (idx + 1) % total_tasks;
        return Some(task.clone());
    }
    idx = (idx + 1) % total_tasks;
}
None
```

#### Schedule

```rust
pub fn schedule(&self) -> Option<TaskCB>
```

Selects next task and marks it `Running`.

### Scheduler (Advanced)

```rust
pub struct Scheduler {
    tasks: Vec<TaskCB>,
    current: usize,
}

static GLOBAL_SCHEDULER: spin::Once<spin::Mutex<Scheduler>> = spin::Once::new();
```

**Purpose**: More sophisticated scheduler with priority-based scheduling.

#### Initialization

```rust
pub fn init_global() {
    GLOBAL_SCHEDULER.call_once(|| {
        spin::Mutex::new(Scheduler::new())
    });
}

pub fn global() -> &'static spin::Mutex<Scheduler> {
    GLOBAL_SCHEDULER.get().expect("Scheduler not initialized")
}
```

#### Starting Scheduler

```rust
pub unsafe fn start() -> !
```

**Purpose**: Begins executing tasks (never returns).

**Process**:
1. Lock scheduler
2. Get first task
3. Mark it `Running`
4. Drop lock (important!)
5. Context switch to task

**Why Drop Lock?**
- Context switch transfers control to task
- Task may call `task_yield()` which locks scheduler
- Would deadlock if lock still held

#### Task Yielding

```rust
pub fn task_yield()
```

**Purpose**: Voluntarily give up CPU to another task (cooperative multitasking).

**Process**:
1. Lock scheduler
2. Find next ready task
3. Update states: current → Ready, next → Running
4. Get raw pointers to contexts
5. Drop lock
6. Context switch

**Preemption** (Future): Timer interrupt calls `task_yield()` for preemptive multitasking.

## Context Switching

### Context Switch Function

```rust
#[naked]
pub unsafe extern "C" fn context_switch(
    old: *mut CPUContext,
    new: *const CPUContext
)
```

**Purpose**: Saves current CPU state to `old`, loads CPU state from `new`.

**Calling Convention**: Uses C calling convention for register passing:
- `rdi` = `old` (pointer to current task's context)
- `rsi` = `new` (pointer to next task's context)

**Naked Function**: No function prologue/epilogue (no stack frame setup).

### Assembly Implementation

The context switch is implemented in pure assembly for maximum control:

#### Save Phase

```asm
// Save callee-saved registers
mov [rdi + 0], rsp    // Save stack pointer
mov [rdi + 8], rbp    // Save base pointer
mov [rdi + 16], rbx
mov [rdi + 24], r12
mov [rdi + 32], r13
mov [rdi + 40], r14
mov [rdi + 48], r15

// Save return address (RIP)
mov rax, [rsp]
mov [rdi + 56], rax

// Save RFLAGS
pushfq
pop rax
mov [rdi + 64], rax

// Save segment selectors
mov ax, cs
mov [rdi + 72], rax
mov ax, ss
mov [rdi + 80], rax
// ... more segments

// Save FS_BASE, GS_BASE (MSRs)
mov ecx, 0xC0000100   // FS_BASE MSR
rdmsr
shl rdx, 32
or rax, rdx
mov [rdi + 120], rax

mov ecx, 0xC0000101   // GS_BASE MSR
rdmsr
shl rdx, 32
or rax, rdx
mov [rdi + 128], rax

// Save CR3 (page table)
mov rax, cr3
mov [rdi + 136], rax
```

#### Restore Phase

```asm
// Restore callee-saved registers
mov rsp, [rsi + 0]
mov rbp, [rsi + 8]
mov rbx, [rsi + 16]
mov r12, [rsi + 24]
mov r13, [rsi + 32]
mov r14, [rsi + 40]
mov r15, [rsi + 48]

// Restore segment selectors
mov ax, [rsi + 104]
mov ds, ax
mov ax, [rsi + 112]
mov es, ax
// ... more segments

// Restore FS_BASE, GS_BASE
mov ecx, 0xC0000100
mov rax, [rsi + 120]
mov rdx, rax
shr rdx, 32
wrmsr

mov ecx, 0xC0000101
mov rax, [rsi + 128]
mov rdx, rax
shr rdx, 32
wrmsr

// Restore CR3 (page table)
mov rax, [rsi + 136]
mov cr3, rax

// Restore RFLAGS
mov rax, [rsi + 64]
push rax
popfq

// Jump to new task's RIP
push qword ptr [rsi + 56]
ret
```

**Key Points**:

1. **Register Offsets**: Correspond to `CPUContext` struct layout
2. **RDMSR/WRMSR**: Read/write model-specific registers (FS_BASE, GS_BASE)
3. **CR3 Switch**: Changes page table (memory isolation between tasks)
4. **Return**: Pushes new RIP and uses `ret` to jump to it

### Context Switch Cost

**Approximate Cycle Counts**:
- Register save/restore: ~50 cycles
- MSR read/write (FS_BASE, GS_BASE): ~200 cycles
- CR3 switch (TLB flush): ~1000 cycles
- Total: ~1250 cycles ≈ 0.4 µs @ 3 GHz

**Optimization**: If tasks share same CR3 (same address space), skip CR3 switch.

## Async Task Support (Prototype)

### Async Task Trait

```rust
pub trait AsyncTask {
    type Output;
    fn poll(&mut self) -> TaskPoll<Self::Output>;
}

pub enum TaskPoll<T> {
    Ready(T),
    Pending,
}
```

**Purpose**: Foundation for async/await support.

**Future Integration**: Combine with Rust's `Future` trait.

### Example Async Task

```rust
pub struct AsyncTaskExample {
    counter: u64,
    target: u64,
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
```

**Usage**:
```rust
let task = AsyncTaskExample::new(1000);
task_manager.spawn_async(task);
```

## Thread Safety

### Scheduler Lock

All scheduler operations hold a mutex:

```rust
let mut scheduler = Scheduler::global().lock();
// Critical section
drop(scheduler);
```

**Deadlock Prevention**:
- Never hold lock during context switch
- Drop lock before calling external functions
- Never acquire multiple locks in different orders

### Lock-Free Operations (Future)

For better performance:

```rust
use core::sync::atomic::{AtomicUsize, Ordering};

struct LockFreeScheduler {
    current: AtomicUsize,
    tasks: [AtomicPtr<TaskCB>; MAX_TASKS],
}
```

## Debugging

### Task State Dump

```rust
pub fn dump_task(task: &TaskCB) {
    serial_println!("Task {}:", task.name);
    serial_println!("  ID: {}", task.id.as_u64());
    serial_println!("  State: {:?}", task.state);
    serial_println!("  Priority: {}", task.priority());
    serial_println!("  RIP: {:#x}", task.context.rip);
    serial_println!("  RSP: {:#x}", task.context.rsp);
    serial_println!("  CR3: {:#x}", task.context.cr3);
}
```

### Scheduler State Dump

```rust
pub fn dump_scheduler() {
    let scheduler = Scheduler::global().lock();
    serial_println!("Scheduler state:");
    serial_println!("  Total tasks: {}", scheduler.task_count());
    serial_println!("  Current task: {}", scheduler.current);
    
    for (i, task) in scheduler.tasks.iter().enumerate() {
        serial_println!("  Task {}: {} ({:?})", i, task.name, task.state);
    }
}
```

### Context Switch Tracing

```rust
pub unsafe fn context_switch_trace(old: *mut CPUContext, new: *const CPUContext) {
    serial_println!("Context switch:");
    serial_println!("  Old RIP: {:#x}", (*old).rip);
    serial_println!("  New RIP: {:#x}", (*new).rip);
    context_switch(old, new);
}
```

## Performance Considerations

### Context Switch Optimization

**Minimize CR3 Switches**:
```rust
if old.context.cr3 == new.context.cr3 {
    context_switch_same_address_space(old, new);  // Skip CR3 load
} else {
    context_switch(old, new);  // Full switch
}
```

**Lazy FPU State**:
```rust
// Don't save/restore FPU registers unless task uses them
if task.uses_fpu {
    save_fpu_state(&mut task.fpu_context);
}
```

### Scheduler Efficiency

**O(1) Scheduler** (Future):
```rust
struct O1Scheduler {
    active: [PriorityQueue; 140],   // Active task queues
    expired: [PriorityQueue; 140],  // Expired task queues
}
```

**Algorithm**:
1. Pick highest-priority non-empty active queue
2. Run task from that queue
3. When time slice expires, move to expired queue
4. When active queues empty, swap active ↔ expired

## Future Enhancements

### SMP Support

```rust
pub struct PerCpuScheduler {
    cpu_id: u8,
    current: Option<TaskId>,
    runqueue: Vec<TaskCB>,
}

// Load balancing
pub fn balance_load(from_cpu: u8, to_cpu: u8);
```

### Priority Inheritance

```rust
// Task A (low priority) holds lock
// Task B (high priority) waits for lock
// → Temporarily raise A's priority to B's level
```

### CPU Affinity

```rust
pub struct TaskCB {
    // ...
    pub cpu_affinity: CpuSet,  // Which CPUs this task can run on
}
```

### Cgroups (Control Groups)

```rust
pub struct CGroup {
    pub cpu_quota: u64,    // CPU time limit
    pub mem_limit: u64,    // Memory limit
    pub tasks: Vec<TaskId>,
}
```

## Dependencies

### Internal Crates

- **hal**: CPU control, serial output
- **x86_64**: CPU register abstractions

### External Crates

- **spin** (0.10.0): Mutex for scheduler

## Configuration

### Cargo.toml

```toml
[package]
name = "task"
version = "0.1.0"
edition = "2024"

[dependencies]
x86_64 = "0.15.2"
spin = "0.10.0"
hal = { path = "../hal" }
```

## References

- [Intel 64 and IA-32 Architectures Software Developer's Manual](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [Linux Completely Fair Scheduler](https://www.kernel.org/doc/html/latest/scheduler/sched-design-CFS.html)
- [OSDev - Scheduling Algorithms](https://wiki.osdev.org/Scheduling_Algorithms)
- [OSDev - Context Switching](https://wiki.osdev.org/Context_Switching)

## License

GPL-3.0 (see LICENSE file in repository root)
