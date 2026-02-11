============================
Serix Kernel API Reference
============================

:Author: Serix Kernel Team
:Version: 0.0.5
:Date: 2025-01-13
:Architecture: x86_64

Overview
========

This document provides the internal API reference for Serix kernel subsystems.
It specifies the contracts between kernel modules including memory management,
task scheduling, interrupt handling, hardware abstraction, and system calls.

The Serix kernel is a microkernel-style operating system written in Rust,
featuring capability-based security and a workspace-based cargo architecture.

Design Principles
-----------------

Type Safety
    Leverage Rust's type system for compile-time safety guarantees

Minimal Unsafe
    Isolate unsafe operations to specific hardware abstraction modules

Zero-Cost Abstractions
    Ensure abstractions compile to optimal machine code with no runtime overhead

Clear Ownership
    Explicit ownership and borrowing semantics throughout the codebase

Error Handling
    Use Result and Option types where appropriate; panic on unrecoverable errors

Subsystem Architecture
----------------------

The kernel consists of independent workspace crates::

    kernel/         Entry point, syscalls, global initialization
    memory/         Page tables, heap allocator, frame allocation
    hal/            Hardware abstraction (serial, CPU topology)
    apic/           APIC interrupt controller (Local APIC, I/O APIC, timer)
    idt/            Interrupt Descriptor Table management
    graphics/       Framebuffer console and drawing primitives
    task/           Async executor, scheduler, task control blocks
    capability/     Capability-based security system
    drivers/        Device drivers (VirtIO, PCI, console)
    vfs/            Virtual filesystem (ramdisk, INode abstraction)
    ipc/            Inter-process communication
    loader/         ELF userspace binary loader
    ulib/           Userspace library (syscall wrappers)

Current Status (v0.0.5)
-----------------------

System Calls
    Basic syscalls implemented: serix_write, serix_read, serix_exit, serix_yield

VFS
    Initialized with ramdisk support, basic file operations

Task Scheduler
    Minimal skeletal implementation, no preemptive scheduling yet

Userspace
    Init binary loads and executes from ramdisk

Memory Management
    Working page tables, heap allocator, frame allocation

Interrupts
    IDT loaded, APIC enabled, timer and keyboard interrupts functional


Memory Management
=================

:Module: memory
:Files: memory/src/lib.rs, memory/src/heap.rs

The memory subsystem provides page table management, heap allocation, and
physical frame allocation. All physical RAM is mapped at virtual offset
0xFFFF_8000_0000_0000 (HHDM - Higher Half Direct Map).

Page Table Management
---------------------

init_offset_page_table()
~~~~~~~~~~~~~~~~~~~~~~~~

Initialize offset page table mapper with active page table::

    pub unsafe fn init_offset_page_table(offset: VirtAddr) -> OffsetPageTable<'static>

Parameters:
    offset
        Virtual address offset for physical memory mapping, typically
        0xFFFF_8000_0000_0000 from Limine HHDM response

Returns:
    OffsetPageTable<'static> mapper instance

Safety:
    - Caller must ensure offset correctly maps physical memory
    - Must be called only once during boot
    - Requires valid CR3 register value

Example usage::

    let phys_mem_offset = VirtAddr::new(0xFFFF_8000_0000_0000);
    let mut mapper = unsafe { memory::init_offset_page_table(phys_mem_offset) };

    // Translate virtual to physical
    use x86_64::structures::paging::Page;
    let page = Page::containing_address(VirtAddr::new(0x4444_4444_0000));
    mapper.translate_page(page);

Notes:
    Returns reference to active level-4 page table. Enables direct physical
    memory access via offset mapping. All physical addresses converted as:
    virt = 0xFFFF_8000_0000_0000 + phys


BootFrameAllocator
~~~~~~~~~~~~~~~~~~

Physical frame allocator using bootloader memory map::

    pub struct BootFrameAllocator {
        frames: &'static [PhysFrame],
        next: usize,
    }

    impl BootFrameAllocator {
        pub fn new(memory_map: &[&Entry]) -> Self
    }

Purpose:
    Allocates physical memory frames (4KiB pages) from usable memory regions
    identified by the bootloader

Trait Implementation::

    unsafe impl FrameAllocator<Size4KiB> for BootFrameAllocator {
        fn allocate_frame(&mut self) -> Option<PhysFrame>
    }

Algorithm:
    Simple bump allocator, no deallocation support

Example usage::

    let mmap_response = MMAP_REQ.get_response().expect("No memory map");
    let entries = mmap_response.entries();
    let mut frame_allocator = BootFrameAllocator::new(entries);

    // Allocate a frame
    if let Some(frame) = frame_allocator.allocate_frame() {
        serial_println!("Allocated frame at: {:?}", frame.start_address());
    }

StaticBootFrameAllocator
~~~~~~~~~~~~~~~~~~~~~~~~~

Pre-heap frame allocator using static array storage::

    pub struct StaticBootFrameAllocator {
        next: usize,
        limit: usize,
    }

    impl StaticBootFrameAllocator {
        pub fn new(frame_count: usize) -> Self
    }

Purpose:
    Frame allocator that works before heap initialization, stores frames
    in static array

Storage::

    pub static mut BOOT_FRAMES: [Option<PhysFrame>; MAX_BOOT_FRAMES] = 
        [None; MAX_BOOT_FRAMES];
    pub const MAX_BOOT_FRAMES: usize = 65536;

Example usage::

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
    memory::init_heap(&mut mapper, &mut frame_alloc);

Heap Management
---------------

init_heap()
~~~~~~~~~~~

Map and initialize kernel heap for dynamic memory allocation::

    pub fn init_heap(
        mapper: &mut OffsetPageTable,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    )

Parameters:
    mapper
        Page table mapper for creating virtual mappings

    frame_allocator
        Allocator for obtaining physical frames

Side Effects:
    - Maps heap region (0xFFFF_8000_4444_0000 to 0xFFFF_8000_4454_0000)
    - Initializes global heap allocator
    - Enables Rust alloc crate functionality (Vec, Box, String)

Panics:
    If frame allocation fails or mapping fails

Constants::

    pub const HEAP_START: usize = 0xFFFF_8000_4444_0000;
    pub const HEAP_SIZE: usize = 1024 * 1024;  // 1 MiB

Example usage::

    // After initializing mapper and frame allocator
    memory::init_heap(&mut mapper, &mut frame_alloc);

    // Now heap allocations work
    extern crate alloc;
    use alloc::vec::Vec;
    let mut v = Vec::new();
    v.push(1);
    v.push(2);

.. note::
    Must be called before any heap allocations (Vec, Box, String).
    Heap initialization is a one-time operation during boot.

Address Translation
-------------------

Helper functions for physical-virtual address conversion::

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

Task Management
===============

:Module: task
:Files: task/src/lib.rs, task/src/context_switch.rs

The task subsystem provides async-based task execution, scheduling primitives,
and context switching. Current implementation is minimal (v0.0.5) with no
preemptive scheduling.

.. asciinema:: task-api-demo.cast
    :title: Task API Demo - Task Creation and Scheduling
    :alt: Demonstration showing task creation with TaskBuilder, adding tasks
          to scheduler, and basic cooperative scheduling via task_yield().
          Shows serial output with task IDs and state transitions.
          Duration: ~45 seconds.

Task Identification
-------------------

TaskId
~~~~~~

Unique identifier for tasks::

    pub struct TaskId(pub u64);

    impl TaskId {
        pub fn new() -> Self
        pub fn as_u64(self) -> u64
    }

Thread Safety:
    Uses atomic counter, safe to call from multiple contexts

Example usage::

    let task_id = TaskId::new();
    serial_println!("Created task ID: {}", task_id.as_u64());

Task State
----------

Task lifecycle states::

    pub enum TaskState {
        Ready,       // Ready to run, waiting for CPU
        Running,     // Currently executing
        Blocked,     // Waiting for I/O or event
        Terminated,  // Finished execution
    }

Scheduling Classes
------------------

Scheduling policies and priorities::

    pub enum SchedClass {
        Realtime(u8),  // Priority 0-99, FIFO scheduling
        Fair(u8),      // Priority 100-139, CFS-style
        Batch,         // Priority 140, background batch
        Iso,           // Isochronous (multimedia)
    }

    impl Default for SchedClass {
        fn default() -> Self {
            SchedClass::Fair(120)
        }
    }

CPU Context
-----------

Complete CPU state for context switching::

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct CPUContext {
        pub rsp: u64,      // Stack pointer
        pub rbp: u64,      // Base pointer
        pub rbx: u64,      // Callee-saved
        pub r12: u64,      // Callee-saved
        pub r13: u64,      // Callee-saved
        pub r14: u64,      // Callee-saved
        pub r15: u64,      // Callee-saved
        pub rip: u64,      // Instruction pointer
        pub rflags: u64,   // CPU flags
        pub cs: u64,       // Code segment
        pub ss: u64,       // Stack segment
        pub fs: u64,       // FS segment
        pub gs: u64,       // GS segment
        pub ds: u64,       // Data segment
        pub es: u64,       // Extra segment
        pub fs_base: u64,  // FS base address
        pub gs_base: u64,  // GS base address (TLS)
        pub cr3: u64,      // Page table base
    }

Task Control Block
------------------

TaskCB
~~~~~~

Represents a schedulable task::

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

new()
^^^^^

Create new task control block::

    pub fn new(
        name: &'static str,
        entry_point: unsafe extern "C" fn() -> !,
        stack: VirtAddr,
        sched_class: SchedClass
    ) -> Self

Parameters:
    name
        Human-readable task name for debugging

    entry_point
        Function to execute when task runs, must never return

    stack
        Top of kernel stack for this task

    sched_class
        Scheduling policy and priority

Returns:
    Initialized TaskCB in Ready state

Example usage::

    extern "C" fn my_task() -> ! {
        loop {
            serial_println!("Task running");
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

Task Builder
------------

Builder pattern for task creation::

    pub struct TaskBuilder {
        name: &'static str,
        sched_class: SchedClass,
        stack_size: usize,
    }

    impl TaskBuilder {
        pub fn new(name: &'static str) -> Self
        pub fn sched_class(mut self, sched_class: SchedClass) -> Self
        pub fn stack_size(mut self, size: usize) -> Self
        pub fn build_kernel_task(self, entry_point: unsafe extern "C" fn() -> !) 
            -> TaskCB
    }

Example usage::

    let task = TaskBuilder::new("background_worker")
        .sched_class(SchedClass::Batch)
        .stack_size(16384)
        .build_kernel_task(worker_function);

    Scheduler::global().lock().add_task(task);

Scheduler
---------

Global task scheduler::

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

init_global()
~~~~~~~~~~~~~

Initialize global scheduler instance::

    pub fn init_global()

Must be called once during kernel initialization before any tasks are created.

Example::

    Scheduler::init_global();

global()
~~~~~~~~

Access global scheduler::

    pub fn global() -> &'static spin::Mutex<Scheduler>

Returns:
    Reference to global scheduler protected by spinlock

Example::

    let mut sched = Scheduler::global().lock();
    sched.add_task(task);
    serial_println!("Task count: {}", sched.task_count());

add_task()
~~~~~~~~~~

Add task to scheduler run queue::

    pub fn add_task(&mut self, task: TaskCB)

Context Switching
-----------------

context_switch()
~~~~~~~~~~~~~~~~

Low-level context switch between tasks::

    #[naked]
    pub unsafe extern "C" fn context_switch(
        old: *mut CPUContext,
        new: *const CPUContext
    )

Parameters:
    old
        Pointer to save current CPU context

    new
        Pointer to load new CPU context

Safety:
    - Must be called with valid context pointers
    - Pointers must remain valid for entire operation
    - Should only be called by scheduler

Example (internal scheduler use)::

    let old_ctx = &mut tasks[current_idx].context as *mut CPUContext;
    let new_ctx = &tasks[next_idx].context as *const CPUContext;
    unsafe {
        context_switch(old_ctx, new_ctx);
    }

task_yield()
~~~~~~~~~~~~

Voluntarily yield CPU to another task::

    pub fn task_yield()

Example::

    extern "C" fn cooperative_task() -> ! {
        loop {
            do_work();
            task::task_yield();  // Let other tasks run
        }
    }

System Calls
============

:Module: kernel
:Files: kernel/src/syscall.rs, ulib/src/lib.rs

The syscall subsystem provides the interface between userspace programs and
the kernel. Currently implements 4 basic syscalls (v0.0.5).

.. asciinema:: syscall-demo.cast
    :title: Syscall Demo - Userspace System Calls
    :alt: Shows userspace init binary making syscalls. Serial output displays
          syscall numbers and arguments for serix_write (write "Hello" to stdout),
          serix_read (read from stdin), serix_yield (cooperative multitasking),
          and serix_exit (terminate with exit code 0). Demonstrates syscall
          calling convention and kernel syscall handler execution.
          Duration: ~30 seconds.

Syscall Numbers
---------------

Syscall vector assignments::

    const SYS_READ:  u64 = 0;
    const SYS_WRITE: u64 = 1;
    const SYS_YIELD: u64 = 24;
    const SYS_EXIT:  u64 = 60;

Calling Convention
------------------

Syscalls use the SYSCALL instruction (x86_64 fast syscall mechanism)::

    rax     Syscall number
    rdi     Argument 1
    rsi     Argument 2
    rdx     Argument 3
    r10     Argument 4 (rcx is clobbered by SYSCALL)
    r8      Argument 5
    r9      Argument 6

    Return value in rax

serix_write()
-------------

Write bytes to file descriptor::

    pub fn serix_write(fd: u64, buf: *const u8, count: u64) -> u64

Parameters:
    fd
        File descriptor (1 = stdout, 2 = stderr)

    buf
        Pointer to data buffer

    count
        Number of bytes to write

Returns:
    Number of bytes written, or error code

Example (userspace)::

    use ulib::serix_write;

    let msg = b"Hello from userspace\n";
    let written = serix_write(1, msg.as_ptr(), msg.len() as u64);

Kernel Handler::

    // kernel/src/syscall.rs
    fn sys_write(fd: u64, buf: u64, count: u64) -> u64 {
        // Validate fd, write to console/VFS
        // Return bytes written
    }

serix_read()
------------

Read bytes from file descriptor::

    pub fn serix_read(fd: u64, buf: *mut u8, count: u64) -> u64

Parameters:
    fd
        File descriptor (0 = stdin)

    buf
        Pointer to buffer for data

    count
        Maximum bytes to read

Returns:
    Number of bytes read, or error code

Example (userspace)::

    use ulib::serix_read;

    let mut buf = [0u8; 128];
    let read_count = serix_read(0, buf.as_mut_ptr(), 128);

serix_exit()
------------

Terminate current task::

    pub fn serix_exit(code: u64) -> !

Parameters:
    code
        Exit status code

Never returns (task is terminated).

Example (userspace)::

    use ulib::serix_exit;

    serix_exit(0);  // Success

serix_yield()
-------------

Yield CPU to scheduler::

    pub fn serix_yield()

Voluntarily gives up remaining timeslice to allow other tasks to run.

Example (userspace)::

    use ulib::serix_yield;

    loop {
        do_work();
        serix_yield();  // Cooperative multitasking
    }

Interrupt Management
====================

:Module: idt
:Files: idt/src/lib.rs


The IDT subsystem loads interrupt handlers and manages CPU exceptions.

IDT Initialization
------------------

init_idt()
~~~~~~~~~~

Load Interrupt Descriptor Table into CPU::

    pub fn init_idt()

Side Effects:
    - Loads IDT into IDTR register
    - Marks IDT as loaded globally

Must be called during kernel initialization before enabling interrupts.

Example usage::

    idt::init_idt();
    x86_64::instructions::interrupts::enable();  // Now safe

Panics:
    None (will triple fault if IDT entries are invalid)

Dynamic Handler Registration
-----------------------------

register_interrupt_handler()
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Register or update interrupt handler after IDT is loaded::

    pub fn register_interrupt_handler(
        vector: u8,
        handler: extern "x86-interrupt" fn(InterruptStackFrame),
    )

Parameters:
    vector
        Interrupt vector number (32-255 for hardware interrupts)

    handler
        Interrupt handler function following x86-interrupt calling convention

Safety:
    Handler must follow x86-interrupt ABI

Example usage::

    extern "x86-interrupt" fn custom_handler(_frame: InterruptStackFrame) {
        serial_println!("Custom interrupt!");
        unsafe { apic::send_eoi(); }
    }

    unsafe {
        idt::register_interrupt_handler(50, custom_handler);
    }

Reloads IDT automatically if already loaded.

Exception Handlers
------------------

Pre-registered CPU exception handlers:

Divide by Zero (Vector 0)
~~~~~~~~~~~~~~~~~~~~~~~~~~

::

    extern "x86-interrupt" fn divide_by_zero_handler(_stack: InterruptStackFrame)

Triggered By:
    Division by zero or division overflow

Action:
    Calls util::panic::oops("Divide by Zero exception")

Page Fault (Vector 14)
~~~~~~~~~~~~~~~~~~~~~~~

::

    extern "x86-interrupt" fn page_fault_handler(
        stack: InterruptStackFrame,
        err: PageFaultErrorCode
    )

Triggered By:
    Invalid memory access (not present, protection violation, reserved bits)

Information Provided:
    - Faulting address (from CR2 register)
    - Error code (present, write, user, reserved, instruction fetch flags)
    - Instruction pointer from stack frame

Action:
    Logs fault details to serial console and halts

Double Fault (Vector 8)
~~~~~~~~~~~~~~~~~~~~~~~~

::

    extern "x86-interrupt" fn double_fault_handler(
        _stack: InterruptStackFrame,
        _err: u64
    ) -> !

Triggered By:
    Exception during exception handling (system in inconsistent state)

Action:
    Panics immediately, never returns

Hardware Interrupt Handlers
----------------------------

Keyboard (Vector 33)
~~~~~~~~~~~~~~~~~~~~

::

    extern "x86-interrupt" fn keyboard_interrupt_handler(_frame: InterruptStackFrame)

IRQ:
    IRQ1 (PS/2 keyboard controller)

Action:
    1. Read scancode from I/O port 0x60
    2. Call keyboard::handle_scancode(scancode)
    3. Send EOI to APIC

APIC Management
===============

:Module: apic
:Files: apic/src/lib.rs, apic/src/ioapic.rs, apic/src/timer.rs

The APIC subsystem manages the Advanced Programmable Interrupt Controller,
including Local APIC, I/O APIC, and LAPIC timer.

Local APIC
----------

enable()
~~~~~~~~

Enable Local APIC and disable legacy PIC::

    pub unsafe fn enable()

Side Effects:
    - Sets APIC Global Enable bit in IA32_APIC_BASE MSR
    - Sets APIC Software Enable bit in SVR register
    - Remaps and masks all legacy PIC interrupts

Must be called before using any APIC functionality.

Example usage::

    unsafe {
        apic::enable();
    }

send_eoi()
~~~~~~~~~~

Signal End of Interrupt to Local APIC::

    pub unsafe fn send_eoi()

Must be called at end of every hardware interrupt handler (not CPU exceptions).

Example usage::

    extern "x86-interrupt" fn interrupt_handler(_frame: InterruptStackFrame) {
        // Handle interrupt
        handle_device();
        
        // Signal completion to APIC
        unsafe {
            apic::send_eoi();
        }
    }

set_timer()
~~~~~~~~~~~

Configure Local APIC timer (low-level interface)::

    pub unsafe fn set_timer(vector: u8, divide: u32, initial_count: u32)

Parameters:
    vector
        Interrupt vector number (32-255)

    divide
        Divider configuration value (0-11)

    initial_count
        Timer period in bus cycles

Example usage::

    unsafe {
        apic::set_timer(49, 0x3, 100_000);  // ~625 Hz
    }

Note:
    Prefer using apic::timer::init_hardware() for standard timer configuration.

I/O APIC
--------

init_ioapic()
~~~~~~~~~~~~~

Initialize I/O APIC with default IRQ routing::

    pub unsafe fn init_ioapic()

Default Mappings:
    - IRQ0 (PIT timer) → Vector 32
    - IRQ1 (keyboard) → Vector 33

Example usage::

    unsafe {
        apic::ioapic::init_ioapic();
    }

map_irq()
~~~~~~~~~

Map hardware IRQ to interrupt vector::

    pub unsafe fn map_irq(irq: u8, vector: u8)

Parameters:
    irq
        Hardware IRQ number (0-23)

    vector
        Interrupt vector number (32-255)

Example usage::

    unsafe {
        // Map IRQ3 (COM2 serial) to vector 35
        apic::ioapic::map_irq(3, 35);
    }

APIC Timer
----------

register_handler()
~~~~~~~~~~~~~~~~~~

Register timer interrupt handler with IDT::

    pub unsafe fn register_handler()

Must be called before idt::init_idt().

Example usage::

    unsafe {
        apic::enable();
        apic::ioapic::init_ioapic();
        apic::timer::register_handler();  // Before IDT load
    }
    idt::init_idt();

init_hardware()
~~~~~~~~~~~~~~~

Configure and start LAPIC timer hardware::

    pub unsafe fn init_hardware()

Must be called after idt::init_idt() and interrupts enabled.

Configuration:
    - Vector: 49 (0x31)
    - Mode: Periodic
    - Divider: 16
    - Frequency: ~625 Hz

Example usage::

    idt::init_idt();
    x86_64::instructions::interrupts::enable();
    unsafe {
        apic::timer::init_hardware();
    }

ticks()
~~~~~~~

Get timer tick count since boot::

    pub fn ticks() -> u64

Returns:
    Number of timer interrupts since kernel started

Thread Safety:
    Safe to call (reads from static variable)

Example usage::

    let start = apic::timer::ticks();
    do_work();
    let end = apic::timer::ticks();
    serial_println!("Work took {} ticks", end - start);

Hardware Abstraction Layer
===========================

:Module: hal
:Files: hal/src/serial.rs, hal/src/cpu.rs, hal/src/topology.rs

Serial Console
--------------

init_serial()
~~~~~~~~~~~~~

Initialize COM1 serial port::

    pub fn init_serial()

Configuration:
    - Port: COM1 (0x3F8)
    - Baud rate: 38400
    - Data bits: 8
    - Stop bits: 1
    - Parity: None

Must be called first during kernel initialization for debug output.

serial_println!()
~~~~~~~~~~~~~~~~~

Print line to serial console::

    serial_println!("format string", args...)

Macro for formatted output to serial port. Safe to call from interrupt context.

Example usage::

    serial_println!("Kernel booting...");
    serial_println!("APIC ID: {}", apic_id);

Utility Functions
=================

:Module: util
:Files: util/src/panic.rs

Panic Handling
--------------

oops()
~~~~~~

Handle kernel oops (non-recoverable error)::

    pub fn oops(msg: &str) -> !

Parameters:
    msg
        Error message to display

Behavior:
    - Prints message with [KERNEL OOPS] prefix to serial console
    - Enters infinite halt loop
    - Never returns

Example usage::

    if !is_valid(ptr) {
        util::panic::oops("Invalid pointer detected");
    }

Use Cases:
    - CPU exceptions (from exception handlers)
    - Hardware errors
    - Assertion failures
    - Unrecoverable kernel state corruption

halt_loop()
~~~~~~~~~~~

Enter infinite halt loop (low power)::

    pub fn halt_loop() -> !

Behavior:
    - Executes HLT instruction in infinite loop
    - CPU wakes on interrupt, then halts again
    - Never returns

Power Efficiency:
    Uses ~1% CPU vs busy-wait loop at 100%

Example usage::

    serial_println!("System halted");
    halt_loop();

Virtual Filesystem
==================

:Module: vfs
:Files: vfs/src/lib.rs

The VFS subsystem provides filesystem abstraction with ramdisk support (v0.0.5).

INode Operations (basic, v0.0.5)
---------------------------------

File operations through INode interface, ramdisk backend initialized during
boot. Full API under development.

Boot Sequence
=============

Initialization Order
--------------------

Critical subsystem initialization sequence::

    1. HAL (Hardware Abstraction Layer)
       hal::init_serial();

    2. APIC (Interrupt Controller)
       unsafe {
           apic::enable();
           apic::ioapic::init_ioapic();
           apic::timer::register_handler();
       }

    3. IDT (Interrupt Handlers)
       idt::init_idt();

    4. Enable Interrupts
       x86_64::instructions::interrupts::enable();

    5. Timer Start
       unsafe {
           apic::timer::init_hardware();
       }

    6. Memory (Paging and Heap)
       let offset = VirtAddr::new(0xFFFF_8000_0000_0000);
       let mut mapper = unsafe { 
           memory::init_offset_page_table(offset) 
       };
       memory::init_heap(&mut mapper, &mut frame_alloc);

    7. Graphics
       graphics::console::init_console(
           fb.addr(), fb.width(), fb.height(), fb.pitch()
       );

    8. VFS
       vfs::init_ramdisk();

    9. Scheduler
       task::Scheduler::init_global();

   10. Userspace
       loader::load_elf(&init_binary);

.. warning::
    Heap must be initialized before any allocations (Vec, Box, String).
    Interrupts must be enabled after IDT is loaded.
    Serial console should be initialized first for debug output.

Data Flow Examples
------------------

Keyboard Input Flow
~~~~~~~~~~~~~~~~~~~

::

    PS/2 Hardware → I/O Port 0x60 → I/O APIC → LAPIC
        ↓
    Keyboard Interrupt (Vector 33, IDT)
        ↓
    keyboard::handle_scancode()
        ↓
        ├─→ hal::serial_print!() → Serial Console (COM1)
        └─→ graphics::fb_print!() → Framebuffer Console

Timer Tick Flow
~~~~~~~~~~~~~~~

::

    LAPIC Timer → Timer Interrupt (Vector 49, IDT)
        ↓
    apic::timer::timer_interrupt()
        ↓
        ├─→ Increment TICKS counter (static atomic)
        └─→ apic::send_eoi()

Page Fault Flow
~~~~~~~~~~~~~~~

::

    Invalid Memory Access → CPU Exception #14
        ↓
    idt::page_fault_handler()
        ↓
        ├─→ Read CR2 register (faulting address)
        ├─→ Parse error code (present, write, user flags)
        ├─→ hal::serial_println!() (log error details)
        └─→ util::panic::oops() → halt_loop()

Synchronization
===============

Thread Safety Guarantees
-------------------------

Single-Core Assumptions:
    Current Serix assumes single-core BSP only:
    - No SMP support
    - No true parallel execution
    - Interrupts provide only concurrency

Synchronization Primitives
---------------------------

Spin Mutex
~~~~~~~~~~

::

    use spin::Mutex;

    static DATA: Mutex<u32> = Mutex::new(0);

    // Usage
    let mut data = DATA.lock();
    *data += 1;
    // Lock automatically released when guard drops

Once Initialization
~~~~~~~~~~~~~~~~~~~

::

    use spin::Once;

    static INIT: Once<MyStruct> = Once::new();

    fn get_instance() -> &'static MyStruct {
        INIT.call_once(|| MyStruct::new())
    }

Interrupt Safety
----------------

Disable interrupts for critical sections::

    x86_64::instructions::interrupts::without_interrupts(|| {
        // Critical section - interrupts disabled
        let mut data = SHARED_DATA.lock();
        data.modify();
    }); // Interrupts restored here

Common Types and Constants
===========================

Type Aliases
------------

::

    use x86_64::{VirtAddr, PhysAddr};
    use x86_64::structures::paging::{Page, PhysFrame, Size4KiB};
    use x86_64::structures::idt::InterruptStackFrame;

Memory Constants
----------------

::

    // Physical memory mapping
    const PHYS_MEM_OFFSET: u64 = 0xFFFF_8000_0000_0000;  // HHDM offset
    
    // Kernel heap
    const HEAP_START: usize = 0xFFFF_8000_4444_0000;
    const HEAP_SIZE: usize = 1024 * 1024;  // 1 MiB
    const MAX_BOOT_FRAMES: usize = 65536;

APIC Constants
--------------

::

    const APIC_BASE: u64 = 0xFEE00000;        // Local APIC MMIO base
    const IOAPIC_BASE: u64 = 0xFEC00000;      // I/O APIC MMIO base

Interrupt Vectors
-----------------

::

    // CPU exceptions: 0-31
    const DIV_BY_ZERO_VECTOR: u8 = 0;
    const DEBUG_VECTOR: u8 = 1;
    const PAGE_FAULT_VECTOR: u8 = 14;
    const DOUBLE_FAULT_VECTOR: u8 = 8;
    
    // Hardware interrupts: 32-255
    const PIT_TIMER_VECTOR: u8 = 32;    // Legacy (disabled)
    const KEYBOARD_VECTOR: u8 = 33;     // PS/2 keyboard
    const TIMER_VECTOR: u8 = 49;        // LAPIC timer

Calling Conventions
-------------------

Rust (default)
~~~~~~~~~~~~~~

Default for Rust-to-Rust calls::

    pub fn rust_function(arg: u32) -> u64 { }

C
~

For bootloader interface and assembly interop::

    pub extern "C" fn c_function(arg: u32) -> u64 { }

x86-interrupt
~~~~~~~~~~~~~

For CPU exception and hardware interrupt handlers::

    extern "x86-interrupt" fn handler(frame: InterruptStackFrame) { }

    extern "x86-interrupt" fn handler_with_error(
        frame: InterruptStackFrame,
        error_code: u64
    ) { }

Error Handling
==============

Current Patterns
----------------

Panic
~~~~~

Used for unrecoverable programming errors::

    value.expect("Description of requirement")
    value.unwrap()
    panic!("Error message")

Oops
~~~~

Used for hardware/CPU exceptions::

    util::panic::oops("Exception description")

Option
~~~~~~

Used for nullable values::

    if let Some(value) = optional_value {
        // Use value
    } else {
        // Handle absence
    }

Future: Result-Based APIs
-------------------------

Planned for post-v1.0::

    pub enum KernelError {
        OutOfMemory,
        InvalidAddress,
        PermissionDenied,
        HardwareError,
        NotFound,
    }

    pub type KernelResult<T> = Result<T, KernelError>;

    pub fn allocate_frame() -> KernelResult<PhysFrame> {
        match FRAME_ALLOCATOR.allocate() {
            Some(frame) => Ok(frame),
            None => Err(KernelError::OutOfMemory),
        }
    }

Future Extensions
=================

Planned API Additions
---------------------

Memory Management
~~~~~~~~~~~~~~~~~

- deallocate_frame() - Free physical frames
- map_range() - Map multiple pages at once
- protect_page() - Change page permissions (RO, RW, NX)
- Memory statistics and pressure tracking

Task Management
~~~~~~~~~~~~~~~

- Preemptive scheduling with timer-based preemption
- task_sleep() - Sleep for duration
- task_block() - Block on condition variable
- task_wake() - Wake blocked task
- Process and thread creation API
- Priority inheritance for mutexes

Interrupt Management
~~~~~~~~~~~~~~~~~~~~

- register_irq_handler() - High-level IRQ registration
- mask_irq() / unmask_irq() - Dynamic IRQ control
- Interrupt statistics and profiling
- MSI/MSI-X support

Device Management
~~~~~~~~~~~~~~~~~

- PCI enumeration and configuration space access
- Device driver registration framework
- DMA buffer allocation with IOMMU support
- Power management (ACPI)

System Calls
~~~~~~~~~~~~

- File I/O: open, close, read, write, seek, stat
- Process management: fork, exec, wait, kill
- Memory: mmap, munmap, mprotect, brk
- IPC: pipe, socket, sendmsg, recvmsg
- Signals: sigaction, kill, sigreturn

API Stability
=============

Current Status:
    Pre-alpha (v0.0.5), APIs subject to change without notice

Versioning:
    Not yet established, will follow semantic versioning post-v1.0

Deprecation Policy:
    Not yet established

API Review:
    Comprehensive review required before v1.0 release

See Also
========

- MEMORY_LAYOUT.md - Complete memory map documentation
- CONTRIBUTING.md - Development guidelines
- Limine Protocol: https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md

---

**End of Document**
