# IDT (Interrupt Descriptor Table) Module

## Overview

The IDT module provides interrupt and exception handling infrastructure for the Serix kernel. It manages the x86_64 Interrupt Descriptor Table (IDT), which maps interrupt vectors to handler functions. This module is critical for handling CPU exceptions (page faults, divide by zero, etc.) and hardware interrupts (keyboard, timer, etc.).

## Architecture

### What is the IDT?

The Interrupt Descriptor Table is a data structure used by x86_64 processors to determine how to respond to interrupts and exceptions. When an interrupt occurs, the CPU:

1. Looks up the interrupt vector (0-255) in the IDT
2. Loads the handler address from the corresponding entry
3. Switches to the handler's privilege level
4. Pushes the current state onto the stack
5. Jumps to the handler function

### IDT Entry Format

Each IDT entry (16 bytes on x86_64) contains:
- **Offset**: 64-bit address of the handler function
- **Segment Selector**: Code segment selector (typically kernel code segment)
- **IST**: Interrupt Stack Table index (0 = don't use IST)
- **Type and Attributes**: Gate type (interrupt/trap), DPL (privilege level)

### Interrupt Vectors

```
0-31:   CPU Exceptions (reserved by architecture)
32-255: User-Defined (hardware interrupts, software interrupts)
```

#### CPU Exception Vectors (0-31)

| Vector | Exception | Description |
|--------|-----------|-------------|
| 0 | #DE | Divide Error |
| 1 | #DB | Debug Exception |
| 2 | NMI | Non-Maskable Interrupt |
| 3 | #BP | Breakpoint |
| 4 | #OF | Overflow |
| 5 | #BR | BOUND Range Exceeded |
| 6 | #UD | Invalid Opcode |
| 7 | #NM | Device Not Available |
| 8 | #DF | Double Fault |
| 9 | - | Coprocessor Segment Overrun (legacy) |
| 10 | #TS | Invalid TSS |
| 11 | #NP | Segment Not Present |
| 12 | #SS | Stack-Segment Fault |
| 13 | #GP | General Protection Fault |
| 14 | #PF | Page Fault |
| 15 | - | Reserved |
| 16 | #MF | x87 FPU Error |
| 17 | #AC | Alignment Check |
| 18 | #MC | Machine Check |
| 19 | #XM | SIMD Floating-Point Exception |
| 20 | #VE | Virtualization Exception |
| 21-31 | - | Reserved |

#### Hardware Interrupt Vectors (32+)

```
32 (0x20): Timer (PIT or LAPIC timer, currently 0x31 for LAPIC)
33 (0x21): Keyboard (PS/2 keyboard, IRQ1)
34-255:    Available for other hardware or software interrupts
```

## Module Structure

```
idt/
├── src/
│   └── lib.rs      # IDT definition, handlers, initialization
└── Cargo.toml
```

## Implementation

### IDT Wrapper

```rust
struct IdtWrapper {
    idt: UnsafeCell<InterruptDescriptorTable>,
    loaded: UnsafeCell<bool>,
}

unsafe impl Sync for IdtWrapper {}
```

**Purpose**: Wraps the IDT in a static-safe structure.

**Why `UnsafeCell`?**
- Allows interior mutability in static context
- IDT must be modified after initialization (dynamic handler registration)
- `Sync` impl declares thread-safety responsibility to programmer

**Why Track `loaded`?**
- Handlers registered after initialization must reload IDT
- Prevents double-loading during init

### Static IDT Instance

```rust
lazy_static! {
    static ref IDT: IdtWrapper = {
        let mut idt = InterruptDescriptorTable::new();
        
        // Register exception handlers
        idt.divide_error.set_handler_fn(divide_by_zero_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        
        // Register hardware interrupt handlers
        idt[33].set_handler_fn(keyboard_interrupt_handler);
        
        IdtWrapper {
            idt: UnsafeCell::new(idt),
            loaded: UnsafeCell::new(false),
        }
    };
}
```

**`lazy_static!`**: Ensures IDT is initialized on first access, not at compile time.

**Why Lazy?**
- IDT entries reference handler functions (addresses only known at runtime)
- Allows complex initialization logic
- Defers initialization until actually needed

### Exception Handlers

#### Divide by Zero

```rust
extern "x86-interrupt" fn divide_by_zero_handler(_stack: InterruptStackFrame)
```

**Purpose**: Handles division by zero and division overflow exceptions.

**Vector**: 0 (#DE)

**Causes**:
- Integer division by zero: `x / 0`
- Division overflow: `i64::MIN / -1` (result doesn't fit in 64 bits)

**Handler Behavior**:
```rust
util::panic::oops("Divide by Zero exception");
```

Prints error message and halts the system.

**Stack Frame**: Contains CPU state at time of exception:
- Instruction pointer (RIP)
- Code segment (CS)
- CPU flags (RFLAGS)
- Stack pointer (RSP)
- Stack segment (SS)

#### Page Fault

```rust
extern "x86-interrupt" fn page_fault_handler(
    stack: InterruptStackFrame,
    err: PageFaultErrorCode
)
```

**Purpose**: Handles page fault exceptions (invalid memory access).

**Vector**: 14 (#PF)

**Causes**:
- Accessing unmapped memory
- Writing to read-only page
- Executing non-executable page
- Ring-3 accessing kernel page (supervisor bit violation)

**Error Code Bits**:
- Bit 0 (P): 0 = Not present, 1 = Protection violation
- Bit 1 (W/R): 0 = Read, 1 = Write
- Bit 2 (U/S): 0 = Supervisor mode, 1 = User mode
- Bit 3 (RSVD): 1 = Reserved bit set in page table
- Bit 4 (I/D): 1 = Instruction fetch

**Handler Implementation**:
```rust
serial_println!(
    "Page fault at instruction pointer: {:#x}",
    stack.instruction_pointer.as_u64()
);

// Read CR2 register (contains faulting address)
let cr2: u64;
unsafe {
    core::arch::asm!("mov {}, cr2", out(reg) cr2);
}
serial_println!("Page fault address: {:#x}", cr2);
serial_println!("Error Code: {:?}", err);

util::panic::oops("Page fault exception");
```

**CR2 Register**: Contains the linear address that caused the page fault.

**Debug Information**:
- **Instruction Pointer**: Where the fault occurred
- **Fault Address (CR2)**: What address was accessed
- **Error Code**: Nature of the fault (present, write, user, etc.)

#### Double Fault

```rust
extern "x86-interrupt" fn double_fault_handler(
    _stack: InterruptStackFrame,
    _err: u64
) -> !
```

**Purpose**: Handles double fault exceptions (fault during fault handling).

**Vector**: 8 (#DF)

**Causes**:
- Exception occurs while handling another exception
- IDT entry not present for exception
- Stack overflow during exception handling
- Invalid exception handler address

**Why `-> !`?**
Double fault handler never returns. The system cannot recover from a double fault in most cases.

**Handler Implementation**:
```rust
serial_println!(
    "Double fault at instruction pointer: {:#x}",
    _stack.instruction_pointer.as_u64()
);
panic!("Double fault exception");
```

**Recovery**: Modern kernels use separate exception stacks (IST) to prevent double faults caused by stack overflow. Serix will implement this in the future.

### Hardware Interrupt Handlers

#### Keyboard Interrupt

```rust
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame)
```

**Purpose**: Handles keyboard interrupts (key press/release).

**Vector**: 33 (IRQ 1 remapped)

**Implementation**:
```rust
use x86_64::instructions::port::Port;

// Read scancode from keyboard controller data port
let mut port = Port::new(0x60);
let scancode: u8 = unsafe { port.read() };

// Process scancode (convert to ASCII, handle special keys)
keyboard::handle_scancode(scancode);

// Send EOI to APIC
unsafe {
    const APIC_EOI: *mut u32 = 0xFEE000B0 as *mut u32;
    APIC_EOI.write_volatile(0);
}
```

**Port 0x60**: PS/2 keyboard controller data register
- Read: Returns scancode of last key event
- Scancode must be read or interrupt won't be deasserted

**Scancode Types**:
- **Make code** (bit 7 = 0): Key pressed
- **Break code** (bit 7 = 1): Key released

**EOI (End of Interrupt)**:
Must signal APIC that interrupt is handled, or no more keyboard interrupts will be delivered.

## Initialization

### Loading the IDT

```rust
pub fn init_idt()
```

**Purpose**: Loads the IDT into the CPU.

**Implementation**:
```rust
unsafe {
    (*IDT.idt.get()).load();
    *IDT.loaded.get() = true;
}
```

**LIDT Instruction**: The `load()` method executes the `LIDT` instruction, which:
1. Takes the address and size of the IDT
2. Stores them in the IDTR (IDT Register)
3. Makes the IDT active

**IDTR Format**:
```
Bits 0-15:   Limit (size - 1)
Bits 16-79:  Base address (64-bit linear address)
```

**Atomicity**: Loading IDT is atomic from CPU perspective, but interrupts should be disabled during initialization.

### Dynamic Handler Registration

```rust
pub fn register_interrupt_handler(
    vector: u8,
    handler: extern "x86-interrupt" fn(InterruptStackFrame),
)
```

**Purpose**: Registers a handler for a specific interrupt vector after IDT is loaded.

**Use Case**: Timer handler is registered before IDT is loaded, so this function doesn't reload. Used for dynamically loaded drivers.

**Implementation**:
```rust
unsafe {
    let idt = &mut *IDT.idt.get();
    idt[vector].set_handler_fn(handler);
    
    // Reload IDT if it was already loaded
    if *IDT.loaded.get() {
        idt.load();
    }
}
```

**Reload Necessity**: CPU caches IDT entries. After modification, must reload IDT for changes to take effect.

## Handler Function ABI

### `extern "x86-interrupt"`

```rust
extern "x86-interrupt" fn handler(stack_frame: InterruptStackFrame)
```

**Purpose**: Special calling convention for interrupt handlers.

**What it Does**:
1. Preserves all registers (CPU saves subset, handler saves rest)
2. Aligns stack properly (CPU pushed error code may misalign)
3. Uses `IRETQ` instruction to return (not `RET`)
4. Restores RFLAGS from stack

**Stack Layout on Entry**:
```
(Higher addresses)
+------------------+
| SS               | <- Pushed by CPU if privilege change
+------------------+
| RSP              | <- Pushed by CPU if privilege change
+------------------+
| RFLAGS           | <- Always pushed by CPU
+------------------+
| CS               | <- Always pushed by CPU
+------------------+
| RIP              | <- Always pushed by CPU
+------------------+
| Error Code       | <- Pushed by CPU for some exceptions
+------------------+
| (Handler stack)  |
(Lower addresses)
```

**Error Code**: Some exceptions push an error code (page fault, double fault, etc.). The handler signature must match:

```rust
// No error code
extern "x86-interrupt" fn handler(frame: InterruptStackFrame);

// With error code
extern "x86-interrupt" fn handler(frame: InterruptStackFrame, error_code: u64);

// With error code (page fault uses typed error code)
extern "x86-interrupt" fn handler(frame: InterruptStackFrame, error_code: PageFaultErrorCode);

// Diverging (double fault)
extern "x86-interrupt" fn handler(frame: InterruptStackFrame, error_code: u64) -> !;
```

### InterruptStackFrame

```rust
pub struct InterruptStackFrame {
    pub instruction_pointer: VirtAddr,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: VirtAddr,
    pub stack_segment: u64,
}
```

**Fields**:
- **instruction_pointer**: Address of interrupted instruction (or next if fault)
- **code_segment**: CS selector at time of interrupt
- **cpu_flags**: RFLAGS register value
- **stack_pointer**: RSP at time of interrupt
- **stack_segment**: SS selector at time of interrupt

**Use Cases**:
- Debugging (where did fault occur?)
- Fault recovery (modify return address to skip faulty instruction)
- Context switching (save/restore CPU state)

## Exception Handling Strategy

### Current Approach: Panic on All Exceptions

```rust
fn exception_handler(...) {
    // Log error information
    serial_println!("Exception occurred: ...");
    
    // Halt system
    util::panic::oops("Exception message");
}
```

**Rationale**:
- Early-stage kernel, exceptions indicate bugs
- No recovery mechanism implemented yet
- Fail fast for easier debugging

### Future: Selective Recovery

#### Recoverable Exceptions

**Page Fault**: Can be handled for:
- Demand paging (allocate page on access)
- Copy-on-write (fork optimization)
- Memory-mapped files
- Swapping

**General Protection Fault**: Can be handled for:
- Invalid user-mode syscall parameters
- User-mode segmentation violations
- Kill offending process instead of kernel panic

#### Non-Recoverable Exceptions

**Double Fault**: System in inconsistent state, cannot recover
**Machine Check**: Hardware error, usually unrecoverable

## Interrupt Safety

### Critical Sections

**Problem**: Interrupt handlers can run at any time, potentially while holding locks.

**Solution**: Disable interrupts during critical sections:

```rust
use x86_64::instructions::interrupts;

interrupts::without_interrupts(|| {
    // Critical section - interrupts disabled
    let mut data = SHARED_DATA.lock();
    data.modify();
}); // Interrupts restored here
```

### Handler Reentrancy

**Problem**: Can an interrupt handler be interrupted by another interrupt?

**Answer**: Yes, unless:
1. Interrupts are disabled in handler (`CLI` instruction)
2. Handler masks its interrupt source
3. Interrupt priority prevents it

**Implications**:
- Handlers should be short and fast
- Handlers should not hold locks for long
- Use lock-free data structures where possible

## Debugging

### Exception Debugging

#### Information to Collect

1. **Exception Vector**: What type of exception?
2. **Instruction Pointer**: Where did it occur?
3. **Error Code**: Additional context (page fault address, etc.)
4. **Stack Trace**: How did we get here?
5. **Register State**: What were registers at time of fault?

#### Page Fault Debugging

```rust
serial_println!("Page Fault!");
serial_println!("  RIP: {:#x}", stack.instruction_pointer.as_u64());
serial_println!("  CR2: {:#x}", read_cr2());
serial_println!("  Error: {:?}", error_code);

if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
    serial_println!("  Protection violation (page was present)");
} else {
    serial_println!("  Page not present");
}

if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
    serial_println!("  Caused by write access");
} else {
    serial_println!("  Caused by read access");
}
```

#### Stack Trace (Future)

```rust
pub unsafe fn print_stack_trace(rbp: u64) {
    let mut frame_ptr = rbp;
    
    for i in 0..10 {
        if frame_ptr == 0 {
            break;
        }
        
        let return_addr = *(frame_ptr as *const u64).offset(1);
        serial_println!("  #{}: {:#x}", i, return_addr);
        
        frame_ptr = *(frame_ptr as *const u64);
    }
}
```

### Interrupt Handler Debugging

#### Interrupt Counting

```rust
static mut IRQ_COUNTS: [u64; 256] = [0; 256];

pub unsafe fn increment_irq_count(vector: u8) {
    IRQ_COUNTS[vector as usize] += 1;
}

pub fn dump_irq_counts() {
    for (i, &count) in IRQ_COUNTS.iter().enumerate() {
        if count > 0 {
            serial_println!("Vector {}: {} interrupts", i, count);
        }
    }
}
```

## Performance Considerations

### Handler Overhead

**Typical Interrupt Latency**: 100-1000 cycles
- CPU state save: 50-100 cycles
- Handler execution: 50-500 cycles (depends on handler)
- CPU state restore: 50-100 cycles
- IRETQ instruction: 50-200 cycles

**Optimization Tips**:
1. Keep handlers short (defer work to bottom half)
2. Avoid memory allocation in handlers
3. Minimize lock contention
4. Use lock-free algorithms where possible

### Exception Overhead

**Exception Cost**: Much higher than interrupt
- Page fault: 1000-10000 cycles
- Includes page table walk, TLB flush, etc.

**Minimize Exceptions**:
1. Pre-fault critical pages (touch them during init)
2. Use large pages (2MB/1GB) to reduce TLB misses
3. Avoid null pointer dereferences
4. Validate pointers before use

## Future Enhancements

### Separate Exception Stacks (IST)

**Problem**: Stack overflow causes double fault because exception handler uses same stack.

**Solution**: Use Interrupt Stack Table (IST) to provide separate stacks for critical exceptions.

```rust
idt.double_fault.set_handler_fn(double_fault_handler)
    .set_stack_index(DOUBLE_FAULT_IST_INDEX);
```

### Interrupt Controller Abstraction

**Goal**: Support multiple interrupt controllers (APIC, MSI, MSI-X)

```rust
pub trait InterruptController {
    unsafe fn eoi(&self, vector: u8);
    unsafe fn mask(&self, vector: u8);
    unsafe fn unmask(&self, vector: u8);
    unsafe fn set_priority(&self, vector: u8, priority: u8);
}
```

### Per-CPU IDTs

**For SMP**: Each CPU needs its own IDT for per-CPU interrupt handling.

### Interrupt Statistics

```rust
pub struct InterruptStats {
    pub count: u64,
    pub total_cycles: u64,
    pub max_cycles: u64,
}
```

## Dependencies

### Internal Crates

- **hal**: Serial output, CPU control
- **util**: Panic handling
- **keyboard**: Scancode processing

### External Crates

- **lazy_static** (1.5.0, features = ["spin_no_std"]): Static initialization
- **x86_64** (0.15.2): IDT abstractions, interrupt stack frame

## Configuration

### Cargo.toml

```toml
[package]
name = "idt"
version = "0.1.0"
edition = "2024"

[dependencies]
x86_64 = "0.15.2"
hal = { path = "../hal" }
util = { path = "../util" }
keyboard = { path = "../keyboard" }
lazy_static = { version = "1.5.0", features = ["spin_no_std"] }
```

## References

- [Intel 64 and IA-32 Architectures Software Developer's Manual, Volume 3A: System Programming Guide, Chapter 6 (Interrupt and Exception Handling)](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [OSDev - Interrupts](https://wiki.osdev.org/Interrupts)
- [OSDev - Exceptions](https://wiki.osdev.org/Exceptions)
- [OSDev - Page Fault](https://wiki.osdev.org/Page_Fault)

## License

GPL-3.0 (see LICENSE file in repository root)
