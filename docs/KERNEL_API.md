# Kernel API Documentation

**Document Version:** 1.0  
**Last Updated:** 2025-10-13  
**Target Architecture:** x86_64  

## Table of Contents

1. [Overview](#overview)
2. [Memory Management API](#memory-management-api)
3. [Task Management API](#task-management-api)
4. [Interrupt Management API](#interrupt-management-api)
5. [APIC API](#apic-api)
6. [Utility API](#utility-api)
7. [Keyboard API](#keyboard-api)
8. [Inter-Subsystem Communication](#inter-subsystem-communication)

---

## Overview

This document specifies the internal APIs used between Serix kernel subsystems. These APIs define the contract between different kernel modules and establish the interfaces for memory management, task scheduling, interrupt handling, and hardware control.

### Design Principles

1. **Type Safety**: Leverage Rust's type system for compile-time safety
2. **Minimal Unsafe**: Unsafe operations isolated to specific modules
3. **Zero-Cost Abstractions**: No runtime overhead for abstractions
4. **Clear Ownership**: Explicit ownership and borrowing semantics
5. **Error Handling**: Use `Result` and `Option` types where appropriate

### Module Dependencies

```
kernel (main)
  ├── memory     (no internal deps)
  ├── hal        (no internal deps)
  ├── util       (depends on: hal)
  ├── idt        (depends on: hal, util, keyboard)
  ├── apic       (depends on: hal, idt, keyboard)
  ├── graphics   (no internal deps)
  ├── keyboard   (depends on: hal, graphics)
  └── task       (depends on: hal)
```

---

## Memory Management API

**Module**: `memory`  
**Path**: `memory/src/lib.rs`, `memory/src/heap.rs`

### Page Table Management

#### init_offset_page_table

```rust
pub unsafe fn init_offset_page_table(offset: VirtAddr) -> OffsetPageTable<'static>
```

**Purpose**: Initializes an offset page table mapper with the active page table.

**Parameters**:
- `offset`: Virtual address offset for physical memory mapping (typically `0xFFFF_8000_0000_0000`)

**Returns**: `OffsetPageTable<'static>` - Page table mapper instance

**Safety**: 
- Caller must ensure offset correctly maps physical memory
- Must be called only once during boot
- Requires valid CR3 register value

**Example**:
```rust
let phys_mem_offset = VirtAddr::new(0xFFFF_8000_0000_0000);
let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };

// Use mapper to map pages
use x86_64::structures::paging::{Page, PageTableFlags};
let page = Page::containing_address(VirtAddr::new(0x4444_4444_0000));
mapper.translate_page(page);
```

**Notes**:
- Returns reference to active level-4 page table
- Enables direct physical memory access via offset mapping
- All physical addresses can be converted: `virt = 0xFFFF_8000_0000_0000 + phys`

#### BootFrameAllocator

```rust
pub struct BootFrameAllocator {
    frames: &'static [PhysFrame],
    next: usize,
}

impl BootFrameAllocator {
    pub fn new(memory_map: &[&Entry]) -> Self
}
```

**Purpose**: Allocates physical memory frames (4KB pages) from usable memory regions.

**Methods**:

##### new

```rust
pub fn new(memory_map: &[&Entry]) -> Self
```

**Parameters**:
- `memory_map`: Slice of memory map entries from bootloader

**Returns**: `BootFrameAllocator` instance

**Example**:
```rust
let mmap_response = MMAP_REQ.get_response().expect("No memory map");
let entries = mmap_response.entries();
let mut frame_allocator = BootFrameAllocator::new(entries);

// Allocate a frame
if let Some(frame) = frame_allocator.allocate_frame() {
    println!("Allocated frame at: {:?}", frame.start_address());
}
```

**Trait Implementation**: `FrameAllocator<Size4KiB>`

```rust
unsafe impl FrameAllocator<Size4KiB> for BootFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame>
}
```

**Algorithm**: Simple bump allocator (no deallocation)

### Heap Management

#### init_heap

```rust
pub fn init_heap(
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
)
```

**Purpose**: Maps and initializes the kernel heap for dynamic memory allocation.

**Parameters**:
- `mapper`: Page table mapper for creating virtual mappings
- `frame_allocator`: Allocator for obtaining physical frames

**Side Effects**:
- Maps heap region (`0x4444_4444_0000` - `0x4444_4454_0000`)
- Initializes global heap allocator
- Enables Rust `alloc` crate functionality

**Example**:
```rust
// After initializing mapper and frame allocator
init_heap(&mut mapper, &mut frame_alloc);

// Now can use heap allocations
use alloc::vec::Vec;
let mut v = Vec::new();
v.push(1);
v.push(2);
```

**Panics**: If frame allocation fails or mapping fails

**Constants**:
```rust
pub const HEAP_START: usize = 0x4444_4444_0000;
pub const HEAP_SIZE: usize = 1024 * 1024;  // 1 MB
pub const MAX_BOOT_FRAMES: usize = 65536;
```

#### StaticBootFrameAllocator

```rust
pub struct StaticBootFrameAllocator {
    next: usize,
    limit: usize,
}

impl StaticBootFrameAllocator {
    pub fn new(frame_count: usize) -> Self
}
```

**Purpose**: Pre-heap frame allocator using static array storage.

**Parameters**:
- `frame_count`: Number of frames stored in `BOOT_FRAMES` array

**Returns**: `StaticBootFrameAllocator` instance

**Storage**:
```rust
pub static mut BOOT_FRAMES: [Option<PhysFrame>; MAX_BOOT_FRAMES] = [None; MAX_BOOT_FRAMES];
```

**Example**:
```rust
// Preallocate frames to static array
let mut frame_count = 0;
for region in usable_regions {
    for frame in region.frames() {
        unsafe {
            BOOT_FRAMES[frame_count] = Some(frame);
        }
        frame_count += 1;
    }
}

// Create allocator
let mut frame_alloc = StaticBootFrameAllocator::new(frame_count);
```

**Trait Implementation**: `FrameAllocator<Size4KiB>`

### Address Translation

```rust
// Convert physical to virtual (kernel space)
pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    const OFFSET: u64 = 0xFFFF_8000_0000_0000;
    VirtAddr::new(OFFSET + phys.as_u64())
}

// Convert virtual to physical (if in physical mapping region)
pub fn virt_to_phys(virt: VirtAddr) -> Option<PhysAddr> {
    const OFFSET: u64 = 0xFFFF_8000_0000_0000;
    if virt.as_u64() >= OFFSET {
        Some(PhysAddr::new(virt.as_u64() - OFFSET))
    } else {
        None
    }
}
```

---

## Task Management API

**Module**: `task`  
**Path**: `task/src/lib.rs`, `task/src/context_switch.rs`

### Task Identification

#### TaskId

```rust
pub struct TaskId(pub u64);

impl TaskId {
    pub fn new() -> Self
    pub fn as_u64(self) -> u64
}
```

**Purpose**: Unique identifier for tasks.

**Methods**:

##### new
```rust
pub fn new() -> Self
```

**Returns**: New unique `TaskId`

**Thread Safety**: Uses atomic counter, safe to call from multiple contexts

**Example**:
```rust
let task_id = TaskId::new();
println!("Created task with ID: {}", task_id.as_u64());
```

### Task State

```rust
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Terminated,
}
```

**States**:
- `Ready`: Task is ready to run, waiting for CPU
- `Running`: Task is currently executing
- `Blocked`: Task is waiting for I/O or event
- `Terminated`: Task has finished execution

### Scheduling Classes

```rust
pub enum SchedClass {
    Realtime(u8),  // Priority 0-99
    Fair(u8),      // Priority 100-139
    Batch,         // Priority 140
    Iso,           // Isochronous
}

impl Default for SchedClass {
    fn default() -> Self {
        SchedClass::Fair(120)
    }
}
```

**Classes**:
- `Realtime(priority)`: High-priority FIFO scheduling
- `Fair(priority)`: CFS-style time-sliced scheduling
- `Batch`: Background batch processing
- `Iso`: Isochronous (multimedia) scheduling

### CPU Context

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CPUContext {
    pub rsp: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cs: u64,
    pub ss: u64,
    pub fs: u64,
    pub gs: u64,
    pub ds: u64,
    pub es: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub cr3: u64,
}
```

**Purpose**: Stores complete CPU state for context switching.

**Fields**:
- Callee-saved registers (RSP, RBP, RBX, R12-R15)
- Execution state (RIP, RFLAGS)
- Segment selectors (CS, SS, DS, ES, FS, GS)
- Segment bases (FS_BASE, GS_BASE)
- Page table base (CR3)

### Task Control Block

```rust
pub struct TaskCB {
    pub id: TaskId,
    pub state: TaskState,
    pub sched_class: SchedClass,
    pub context: CPUContext,
    pub kstack: VirtAddr,
    pub ustack: Option<VirtAddr>,
    pub name: &'static str,
}

impl TaskCB {
    pub fn new(
        name: &'static str,
        entry_point: unsafe extern "C" fn() -> !,
        stack: VirtAddr,
        sched_class: SchedClass
    ) -> Self
    
    pub fn set_state(&mut self, state: TaskState)
    pub fn priority(&self) -> u8
}
```

**Purpose**: Represents a schedulable task.

**Methods**:

##### new
```rust
pub fn new(
    name: &'static str,
    entry_point: unsafe extern "C" fn() -> !,
    stack: VirtAddr,
    sched_class: SchedClass
) -> Self
```

**Parameters**:
- `name`: Human-readable task name
- `entry_point`: Function to execute when task runs
- `stack`: Top of stack for this task
- `sched_class`: Scheduling policy and priority

**Returns**: Initialized `TaskCB` in `Ready` state

**Example**:
```rust
extern "C" fn my_task() -> ! {
    loop {
        // Task code
        task::task_yield();
    }
}

let stack = VirtAddr::new(0xFFFF_8000_0001_0000);
let task = TaskCB::new(
    "my_task",
    my_task,
    stack,
    SchedClass::Fair(120)
);
```

##### set_state
```rust
pub fn set_state(&mut self, state: TaskState)
```

**Purpose**: Changes task state.

##### priority
```rust
pub fn priority(&self) -> u8
```

**Returns**: Numeric priority (0-255, lower is higher priority)

### Task Builder

```rust
pub struct TaskBuilder {
    name: &'static str,
    sched_class: SchedClass,
    stack_size: usize,
}

impl TaskBuilder {
    pub fn new(name: &'static str) -> Self
    pub fn sched_class(mut self, sched_class: SchedClass) -> Self
    pub fn stack_size(mut self, size: usize) -> Self
    pub fn build_kernel_task(self, entry_point: unsafe extern "C" fn() -> !) -> TaskCB
}
```

**Purpose**: Builder pattern for creating tasks.

**Example**:
```rust
let task = TaskBuilder::new("background_worker")
    .sched_class(SchedClass::Batch)
    .stack_size(16384)
    .build_kernel_task(worker_function);

Scheduler::global().lock().add_task(task);
```

### Scheduler

```rust
pub struct Scheduler {
    tasks: Vec<TaskCB>,
    current: usize,
}

impl Scheduler {
    pub fn new() -> Self
    pub fn init_global()
    pub fn global() -> &'static spin::Mutex<Scheduler>
    pub unsafe fn start() -> !
    pub fn add_task(&mut self, task: TaskCB)
    pub fn task_count(&self) -> usize
}
```

**Purpose**: Global task scheduler.

**Methods**:

##### init_global
```rust
pub fn init_global()
```

**Purpose**: Initializes global scheduler instance (must be called once during boot).

**Example**:
```rust
// During kernel initialization
Scheduler::init_global();
```

##### global
```rust
pub fn global() -> &'static spin::Mutex<Scheduler>
```

**Returns**: Reference to global scheduler

**Example**:
```rust
let mut sched = Scheduler::global().lock();
sched.add_task(task);
```

##### start
```rust
pub unsafe fn start() -> !
```

**Purpose**: Begins executing tasks (never returns).

**Safety**: Must be called with valid task list

**Example**:
```rust
// After adding tasks
unsafe {
    Scheduler::start();  // Never returns
}
```

##### add_task
```rust
pub fn add_task(&mut self, task: TaskCB)
```

**Purpose**: Adds task to scheduler's task list.

### Context Switching

```rust
#[naked]
pub unsafe extern "C" fn context_switch(
    old: *mut CPUContext,
    new: *const CPUContext
)
```

**Purpose**: Performs low-level context switch between tasks.

**Parameters**:
- `old`: Pointer to save current CPU context
- `new`: Pointer to load new CPU context

**Safety**: 
- Must be called with valid context pointers
- Pointers must remain valid for entire operation
- Should only be called by scheduler

**Example**:
```rust
// In scheduler
let old_ctx = &mut tasks[current_idx].context as *mut CPUContext;
let new_ctx = &tasks[next_idx].context as *const CPUContext;

unsafe {
    context_switch(old_ctx, new_ctx);
}
```

### Task Yielding

```rust
pub fn task_yield()
```

**Purpose**: Voluntarily yields CPU to another task.

**Example**:
```rust
extern "C" fn cooperative_task() -> ! {
    loop {
        // Do some work
        do_work();
        
        // Yield to other tasks
        task::task_yield();
    }
}
```

### Task Manager

```rust
pub struct TaskManager {
    tasks: Mutex<RefCell<Vec<TaskCB>>>,
    current_task_idx: Mutex<usize>,
}

impl TaskManager {
    pub const fn new() -> Self
    pub fn create_task(name: &'static str) -> TaskBuilder
    pub fn add_task(&self, task: TaskCB)
    pub fn next_ready_task(&self) -> Option<TaskCB>
    pub fn schedule(&self) -> Option<TaskCB>
    pub fn update_task(&self, updated_task: TaskCB)
}
```

**Purpose**: Global task registry (alternative to Scheduler for simple use cases).

---

## Interrupt Management API

**Module**: `idt`  
**Path**: `idt/src/lib.rs`

### IDT Initialization

```rust
pub fn init_idt()
```

**Purpose**: Loads the Interrupt Descriptor Table into the CPU.

**Side Effects**:
- Loads IDT into IDTR register
- Marks IDT as loaded

**Example**:
```rust
// During kernel initialization
idt::init_idt();

// Now interrupts can be enabled
x86_64::instructions::interrupts::enable();
```

**Panics**: None (will triple fault if IDT entries invalid)

### Dynamic Handler Registration

```rust
pub fn register_interrupt_handler(
    vector: u8,
    handler: extern "x86-interrupt" fn(InterruptStackFrame),
)
```

**Purpose**: Registers or updates an interrupt handler after IDT is loaded.

**Parameters**:
- `vector`: Interrupt vector number (32-255 for hardware interrupts)
- `handler`: Interrupt handler function

**Safety**: Handler must follow x86-interrupt calling convention

**Example**:
```rust
extern "x86-interrupt" fn custom_handler(_frame: InterruptStackFrame) {
    serial_println!("Custom interrupt!");
    unsafe { apic::send_eoi(); }
}

// Register handler for vector 50
unsafe {
    idt::register_interrupt_handler(50, custom_handler);
}
```

**Reloads IDT**: Automatically reloads IDT if already loaded

### Exception Handlers

Pre-registered exception handlers:

#### Divide by Zero (Vector 0)

```rust
extern "x86-interrupt" fn divide_by_zero_handler(_stack: InterruptStackFrame)
```

**Triggered By**: Division by zero or division overflow

**Action**: Calls `util::panic::oops("Divide by Zero exception")`

#### Page Fault (Vector 14)

```rust
extern "x86-interrupt" fn page_fault_handler(
    stack: InterruptStackFrame,
    err: PageFaultErrorCode
)
```

**Triggered By**: Invalid memory access

**Information Provided**:
- Faulting address (from CR2 register)
- Error code (present, write, user flags)
- Instruction pointer

**Action**: Logs fault information and halts

#### Double Fault (Vector 8)

```rust
extern "x86-interrupt" fn double_fault_handler(
    _stack: InterruptStackFrame,
    _err: u64
) -> !
```

**Triggered By**: Exception during exception handling

**Action**: Panics (system in inconsistent state)

### Hardware Interrupt Handlers

#### Keyboard (Vector 33)

```rust
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame)
```

**Action**:
1. Reads scancode from port 0x60
2. Calls `keyboard::handle_scancode(scancode)`
3. Sends EOI to APIC

**IRQ**: IRQ1 (PS/2 keyboard)

---

## APIC API

**Module**: `apic`  
**Path**: `apic/src/lib.rs`, `apic/src/ioapic.rs`, `apic/src/timer.rs`

### Local APIC

#### enable

```rust
pub unsafe fn enable()
```

**Purpose**: Enables the Local APIC and disables legacy PIC.

**Side Effects**:
- Sets APIC Global Enable bit in IA32_APIC_BASE MSR
- Sets APIC Software Enable bit in SVR register
- Remaps and masks all PIC interrupts

**Example**:
```rust
unsafe {
    apic::enable();
}
```

**Must Call**: Before using any APIC functionality

#### send_eoi

```rust
pub unsafe fn send_eoi()
```

**Purpose**: Signals End of Interrupt to Local APIC.

**Example**:
```rust
extern "x86-interrupt" fn interrupt_handler(_frame: InterruptStackFrame) {
    // Handle interrupt
    
    // Signal completion
    unsafe {
        apic::send_eoi();
    }
}
```

**Critical**: Must be called at end of every interrupt handler (except CPU exceptions)

#### set_timer

```rust
pub unsafe fn set_timer(vector: u8, divide: u32, initial_count: u32)
```

**Purpose**: Configures Local APIC timer (low-level interface).

**Parameters**:
- `vector`: Interrupt vector number (32-255)
- `divide`: Divider configuration (0-11)
- `initial_count`: Timer period in bus cycles

**Example**:
```rust
unsafe {
    // 625 Hz timer
    apic::set_timer(49, 0x3, 100_000);
}
```

**Note**: Prefer using `apic::timer::init_hardware()` for standard configuration

### I/O APIC

#### init_ioapic

```rust
pub unsafe fn init_ioapic()
```

**Purpose**: Initializes I/O APIC with default IRQ routing.

**Default Mappings**:
- IRQ0 (timer) → Vector 32
- IRQ1 (keyboard) → Vector 33

**Example**:
```rust
unsafe {
    apic::ioapic::init_ioapic();
}
```

#### map_irq

```rust
pub unsafe fn map_irq(irq: u8, vector: u8)
```

**Purpose**: Maps hardware IRQ to interrupt vector.

**Parameters**:
- `irq`: Hardware IRQ number (0-23)
- `vector`: Interrupt vector (32-255)

**Example**:
```rust
unsafe {
    // Map IRQ3 (COM2 serial) to vector 35
    apic::ioapic::map_irq(3, 35);
}
```

### APIC Timer

#### register_handler

```rust
pub unsafe fn register_handler()
```

**Purpose**: Registers timer interrupt handler with IDT.

**Must Call**: Before `idt::init_idt()`

**Example**:
```rust
unsafe {
    apic::enable();
    apic::ioapic::init_ioapic();
    apic::timer::register_handler();  // Before IDT load
}

idt::init_idt();
```

#### init_hardware

```rust
pub unsafe fn init_hardware()
```

**Purpose**: Configures and starts LAPIC timer hardware.

**Must Call**: After `idt::init_idt()` and interrupts enabled

**Example**:
```rust
idt::init_idt();
x86_64::instructions::interrupts::enable();

unsafe {
    apic::timer::init_hardware();  // Start timer
}
```

**Configuration**:
- Vector: 49 (0x31)
- Mode: Periodic
- Divider: 16
- Frequency: ~625 Hz

#### ticks

```rust
pub fn ticks() -> u64
```

**Purpose**: Returns number of timer interrupts since boot.

**Returns**: Tick count (increments at timer frequency)

**Example**:
```rust
let start = apic::timer::ticks();
do_work();
let end = apic::timer::ticks();
let elapsed = end - start;
println!("Work took {} ticks", elapsed);
```

**Thread Safety**: Reads from static variable (atomic not needed for single-core)

---

## Utility API

**Module**: `util`  
**Path**: `util/src/lib.rs`, `util/src/panic.rs`

### Panic Handling

#### oops

```rust
pub fn oops(msg: &str) -> !
```

**Purpose**: Handles kernel oops (non-recoverable error).

**Parameters**:
- `msg`: Error message to display

**Behavior**:
- Prints message to serial console with `[KERNEL OOPS]` prefix
- Enters infinite halt loop (never returns)

**Example**:
```rust
if !is_valid(value) {
    util::panic::oops("Invalid value detected");
}
```

**Use Cases**:
- CPU exceptions (from exception handlers)
- Hardware errors
- Assertion failures
- Unrecoverable kernel errors

#### halt_loop

```rust
pub fn halt_loop() -> !
```

**Purpose**: Enters infinite halt loop (low power).

**Behavior**:
- Executes `HLT` instruction in loop
- CPU wakes on interrupt, then halts again
- Never returns

**Example**:
```rust
// After critical error
serial_println!("System halted");
halt_loop();
```

**Power Efficiency**: Uses ~1% CPU vs busy loop at 100%

---

## Keyboard API

**Module**: `keyboard`  
**Path**: `keyboard/src/lib.rs`

### Scancode Handling

#### handle_scancode

```rust
pub fn handle_scancode(scancode: u8)
```

**Purpose**: Processes keyboard scancode and outputs to consoles.

**Parameters**:
- `scancode`: Raw scancode from keyboard controller (port 0x60)

**Behavior**:
- Ignores break codes (key release)
- Translates make codes to ASCII
- Outputs to serial console
- Outputs to framebuffer console

**Example**:
```rust
extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    let scancode = unsafe { inb(0x60) };
    keyboard::handle_scancode(scancode);
    unsafe { apic::send_eoi(); }
}
```

**Translation**: US QWERTY layout, lowercase only (no modifier support yet)

#### enable_keyboard_interrupt

```rust
pub fn enable_keyboard_interrupt()
```

**Purpose**: Enables keyboard interrupt on PIC (legacy, may not be needed with APIC).

**Example**:
```rust
unsafe {
    keyboard::enable_keyboard_interrupt();
}
```

**Note**: With APIC, `apic::ioapic::init_ioapic()` handles IRQ routing

---

## Inter-Subsystem Communication

### Subsystem Initialization Order

```rust
// kernel/src/main.rs

// 1. HAL (Hardware Abstraction Layer)
hal::init_serial();

// 2. APIC (Interrupt Controller)
unsafe {
    apic::enable();
    apic::ioapic::init_ioapic();
    apic::timer::register_handler();
}

// 3. IDT (Interrupt Handlers)
idt::init_idt();

// 4. Enable Interrupts
x86_64::instructions::interrupts::enable();

// 5. Timer Start
unsafe {
    apic::timer::init_hardware();
}

// 6. Memory (Paging and Heap)
let phys_mem_offset = VirtAddr::new(0xFFFF_8000_0000_0000);
let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };
// ... frame allocation
memory::init_heap(&mut mapper, &mut frame_alloc);

// 7. Graphics
graphics::console::init_console(fb.addr(), fb.width(), fb.height(), fb.pitch());

// 8. Scheduler
task::Scheduler::init_global();
```

### Data Flow Examples

#### Keyboard Input Flow

```
Hardware → Port 0x60 → Keyboard Interrupt (IDT)
    ↓
keyboard::handle_scancode()
    ↓
    ├─→ hal::serial_print!() → Serial Console
    └─→ graphics::fb_print!() → Framebuffer Console
```

#### Timer Tick Flow

```
LAPIC Timer → Timer Interrupt (IDT)
    ↓
apic::timer::timer_interrupt()
    ↓
    ├─→ Increment TICKS counter
    └─→ apic::send_eoi()
```

#### Page Fault Flow

```
Invalid Memory Access → CPU Exception
    ↓
idt::page_fault_handler()
    ↓
    ├─→ Read CR2 (fault address)
    ├─→ hal::serial_println!() (log error)
    └─→ util::panic::oops() → halt_loop()
```

### Shared Resources

#### Global Statics

```rust
// IDT
static ref IDT: IdtWrapper = { /* ... */ };

// Serial Port
static SERIAL_PORT: Once<Mutex<SerialPort>> = Once::new();

// Framebuffer Console
static GLOBAL_CONSOLE: Mutex<Option<FramebufferConsole>> = Mutex::new(None);

// Scheduler
static GLOBAL_SCHEDULER: spin::Once<spin::Mutex<Scheduler>> = spin::Once::new();

// Heap Allocator
#[global_allocator]
pub static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();
```

**Synchronization**: All globals use mutexes or atomic initialization (Once)

---

## Error Handling Patterns

### Result-Based APIs (Future)

```rust
pub enum KernelError {
    OutOfMemory,
    InvalidAddress,
    PermissionDenied,
    HardwareError,
}

pub type KernelResult<T> = Result<T, KernelError>;

// Example usage
pub fn allocate_frame() -> KernelResult<PhysFrame> {
    if let Some(frame) = FRAME_ALLOCATOR.allocate() {
        Ok(frame)
    } else {
        Err(KernelError::OutOfMemory)
    }
}
```

### Current Error Handling

**Panic**: Used for unrecoverable errors
```rust
.expect("Description")
.unwrap()
panic!("Error message")
```

**Oops**: Used for hardware/CPU exceptions
```rust
util::panic::oops("Exception description")
```

**Option**: Used for nullable values
```rust
if let Some(value) = optional_value {
    // Use value
}
```

---

## Thread Safety Guarantees

### Single-Core Assumptions

Current Serix assumes single-core (BSP only):
- No SMP support
- No atomic synchronization beyond spin locks
- Interrupts provide only concurrency

### Synchronization Primitives

#### Spin Mutex

```rust
use spin::Mutex;

static DATA: Mutex<u32> = Mutex::new(0);

// Usage
let mut data = DATA.lock();
*data += 1;
// Lock automatically released when guard drops
```

#### Once Initialization

```rust
use spin::Once;

static INIT: Once<MyStruct> = Once::new();

fn get_instance() -> &'static MyStruct {
    INIT.call_once(|| MyStruct::new())
}
```

### Interrupt Safety

**Disable Interrupts for Critical Sections**:
```rust
x86_64::instructions::interrupts::without_interrupts(|| {
    // Critical section - interrupts disabled
    let mut data = SHARED_DATA.lock();
    data.modify();
}); // Interrupts restored here
```

---

## Future API Extensions

### Planned Additions

#### Memory Management
- `deallocate_frame()` - Free physical frames
- `map_range()` - Map multiple pages at once
- `protect_page()` - Change page permissions
- Memory statistics API

#### Task Management
- `task_sleep()` - Sleep for duration
- `task_block()` - Block on condition
- `task_wake()` - Wake blocked task
- Process creation API
- Thread creation API

#### Interrupt Management
- `register_irq_handler()` - Register IRQ handler
- `mask_irq()` / `unmask_irq()` - IRQ control
- Interrupt statistics API

#### Device Management
- PCI enumeration API
- Device driver registration
- DMA buffer allocation

---

## API Stability

**Current Status**: Pre-alpha, APIs subject to change

**Versioning**: Not yet established

**Deprecation Policy**: Not yet established

**API Review**: Required before 1.0 release

---

## Appendix

### Common Type Aliases

```rust
use x86_64::{VirtAddr, PhysAddr};
use x86_64::structures::paging::{Page, PhysFrame, Size4KiB};
use x86_64::structures::idt::InterruptStackFrame;
```

### Common Constants

```rust
// Memory
const PHYS_MEM_OFFSET: u64 = 0xFFFF_8000_0000_0000;
const HEAP_START: usize = 0x4444_4444_0000;
const HEAP_SIZE: usize = 1024 * 1024;

// APIC
const APIC_BASE: u64 = 0xFEE00000;
const IOAPIC_BASE: u64 = 0xFEC00000;

// Vectors
const KEYBOARD_VECTOR: u8 = 33;
const TIMER_VECTOR: u8 = 49;
```

### Calling Conventions

**Rust**: Default for Rust-to-Rust calls
```rust
pub fn rust_function(arg: u32) -> u64 { /* ... */ }
```

**C**: For bootloader interface and assembly
```rust
pub extern "C" fn c_function(arg: u32) -> u64 { /* ... */ }
```

**x86-interrupt**: For interrupt handlers
```rust
extern "x86-interrupt" fn handler(frame: InterruptStackFrame) { /* ... */ }
```

---

**End of Document**
