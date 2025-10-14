# Interrupt Handling Technical Specification

**Document Version:** 1.0  
**Last Updated:** 2025-10-13  
**Target Architecture:** x86_64  

## Table of Contents

1. [Overview](#overview)
2. [Interrupt Architecture](#interrupt-architecture)
3. [IDT Structure and Setup](#idt-structure-and-setup)
4. [APIC Configuration](#apic-configuration)
5. [Interrupt Processing Flow](#interrupt-processing-flow)
6. [Exception Handlers](#exception-handlers)
7. [Hardware Interrupt Handlers](#hardware-interrupt-handlers)
8. [Interrupt Routing](#interrupt-routing)
9. [Performance and Optimization](#performance-and-optimization)
10. [Debugging and Troubleshooting](#debugging-and-troubleshooting)

---

## Overview

Serix uses a modern interrupt handling infrastructure based on the x86_64 Interrupt Descriptor Table (IDT) and Advanced Programmable Interrupt Controller (APIC). This document specifies the complete interrupt handling mechanism from hardware signal to software handler execution.

### Key Components

| Component | Purpose | Location |
|-----------|---------|----------|
| IDT | Maps interrupt vectors to handlers | IDTR register |
| Local APIC | Per-CPU interrupt controller | MMIO at 0xFEE00000 |
| I/O APIC | Routes hardware IRQs to CPUs | MMIO at 0xFEC00000 |
| Legacy PIC | Disabled, not used | Ports 0x20/0x21, 0xA0/0xA1 |

### Interrupt Vector Allocation

```
Vector Range    Usage                       Handler Location
──────────────────────────────────────────────────────────────
0-31            CPU Exceptions              idt/src/lib.rs
32              Timer (PIT, remapped)       Not actively used
33 (0x21)       Keyboard (IRQ1)             idt/src/lib.rs
34-48           Reserved for future IRQs    Not implemented
49 (0x31)       LAPIC Timer                 apic/src/timer.rs
50-254          Available                   Not implemented
255 (0xFF)      Spurious (APIC)             Not implemented
```

---

## Interrupt Architecture

### x86_64 Interrupt Model

```
Hardware Event
      │
      ▼
  ┌───────────────┐
  │   APIC/CPU    │
  │  Determines   │
  │    Vector     │
  └───────┬───────┘
          │
          ▼
  ┌───────────────┐
  │  CPU looks up │
  │  vector in    │
  │     IDT       │
  └───────┬───────┘
          │
          ▼
  ┌───────────────┐
  │  CPU pushes   │
  │  state to     │
  │    stack      │
  └───────┬───────┘
          │
          ▼
  ┌───────────────┐
  │  Jump to      │
  │  handler      │
  │  address      │
  └───────┬───────┘
          │
          ▼
  ┌───────────────┐
  │  Handler      │
  │  executes     │
  └───────┬───────┘
          │
          ▼
  ┌───────────────┐
  │  Send EOI     │
  │  to APIC      │
  └───────┬───────┘
          │
          ▼
  ┌───────────────┐
  │  IRETQ back   │
  │  to code      │
  └───────────────┘
```

### Interrupt vs Exception

| Aspect | Exception | Interrupt |
|--------|-----------|-----------|
| Source | CPU (synchronous) | Hardware/Software (asynchronous) |
| Timing | Precise (at instruction boundary) | Approximate (between instructions) |
| Vector | 0-31 (fixed) | 32-255 (configurable) |
| Error Code | Some push error code | None (interrupt number only) |
| Examples | Page fault, divide by zero | Timer, keyboard, disk I/O |

---

## IDT Structure and Setup

### Interrupt Descriptor Table (IDT)

**Size**: 256 entries × 16 bytes = 4096 bytes (1 page)

**Location**: Kernel memory, address loaded into IDTR register

**Entry Format** (16 bytes per entry):

```
Bits    Field               Description
────────────────────────────────────────────────────────────────
0-15    Offset Low          Handler address bits [15:0]
16-31   Segment Selector    Code segment (typically 0x08 for kernel)
32-34   IST                 Interrupt Stack Table index (0 = don't use)
35-39   Reserved            Must be zero
40-43   Gate Type           0xE = Interrupt Gate, 0xF = Trap Gate
44      Zero                Must be zero
45-46   DPL                 Descriptor Privilege Level (0 for kernel)
47      Present             Must be 1 for valid entry
48-63   Offset Middle       Handler address bits [31:16]
64-95   Offset High         Handler address bits [63:32]
96-127  Reserved            Must be zero
```

### Gate Types

**Interrupt Gate (0xE)**:
- Automatically disables interrupts (clears IF flag)
- Used for most interrupt handlers
- Prevents reentrant interrupts during handler execution

**Trap Gate (0xF)**:
- Does not disable interrupts (IF flag unchanged)
- Used for debugging and system calls
- Allows interrupts during handler execution

**Serix Policy**: Use interrupt gates for all handlers (safety first).

### IDT Initialization

```rust
// idt/src/lib.rs

use lazy_static::lazy_static;
use x86_64::structures::idt::InterruptDescriptorTable;

lazy_static! {
    static ref IDT: IdtWrapper = {
        let mut idt = InterruptDescriptorTable::new();
        
        // CPU Exceptions (vectors 0-31)
        idt.divide_error.set_handler_fn(divide_by_zero_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        
        // Hardware Interrupts (vectors 32+)
        idt[33].set_handler_fn(keyboard_interrupt_handler);
        
        IdtWrapper {
            idt: UnsafeCell::new(idt),
            loaded: UnsafeCell::new(false),
        }
    };
}

pub fn init_idt() {
    unsafe {
        (*IDT.idt.get()).load();
        *IDT.loaded.get() = true;
    }
}
```

### IDTR Register

**Format** (10 bytes):

```
┌─────────────────────────────────────────────────────┬────────────────┐
│        Base Address (64-bit)                        │  Limit (16-bit)│
├─────────────────────────────────────────────────────┼────────────────┤
│                    Bits 79-16                       │   Bits 15-0    │
└─────────────────────────────────────────────────────┴────────────────┘
```

**Fields**:
- **Limit**: Size of IDT - 1 (typically 4095 for 256 entries)
- **Base**: Linear address of IDT

**Loading IDTR**:
```rust
// Executed by load() method
core::arch::asm!("lidt [{}]", in(reg) &idtr_value);
```

---

## APIC Configuration

### Legacy PIC Disable

**Why Disable?**
- APIC is superior (better multiprocessor support, more IRQs)
- PIC conflicts with APIC if both active
- PIC defaults to vectors 0-15 (overlaps CPU exceptions)

**Procedure** (apic/src/lib.rs):

```rust
pub unsafe fn disable_pic() {
    use x86_64::instructions::port::Port;
    
    let mut pic1_cmd: Port<u8> = Port::new(0x20);
    let mut pic1_data: Port<u8> = Port::new(0x21);
    let mut pic2_cmd: Port<u8> = Port::new(0xA0);
    let mut pic2_data: Port<u8> = Port::new(0xA1);
    
    // Initialize PIC (ICW1-ICW4)
    pic1_cmd.write(0x11);      // ICW1: Start init, cascade mode
    pic2_cmd.write(0x11);
    
    pic1_data.write(0x20);     // ICW2: Remap to vectors 32-39
    pic2_data.write(0x28);     // ICW2: Remap to vectors 40-47
    
    pic1_data.write(0x04);     // ICW3: Slave on IRQ2
    pic2_data.write(0x02);     // ICW3: Cascade identity
    
    pic1_data.write(0x01);     // ICW4: 8086 mode
    pic2_data.write(0x01);
    
    // Mask all interrupts
    pic1_data.write(0xFF);     // Disable all IRQs on master
    pic2_data.write(0xFF);     // Disable all IRQs on slave
}
```

**PIC Ports**:
| Port | PIC | Register |
|------|-----|----------|
| 0x20 | Master | Command |
| 0x21 | Master | Data |
| 0xA0 | Slave | Command |
| 0xA1 | Slave | Data |

### Local APIC Enable

**MMIO Base**: 0xFEE00000 (4 KB region)

**Registers**:

| Offset | Register | Purpose |
|--------|----------|---------|
| 0x020 | ID | Local APIC ID (CPU identifier) |
| 0x030 | Version | APIC version and max LVT entries |
| 0x080 | TPR | Task Priority Register |
| 0x0B0 | EOI | End of Interrupt |
| 0x0F0 | SVR | Spurious Interrupt Vector Register |
| 0x320 | LVT Timer | Timer interrupt configuration |
| 0x350 | LVT LINT0 | External interrupt 0 |
| 0x360 | LVT LINT1 | External interrupt 1 (NMI) |
| 0x370 | LVT Error | Error interrupt |
| 0x380 | Timer Initial Count | Timer countdown value |
| 0x390 | Timer Current Count | Current timer value (read-only) |
| 0x3E0 | Timer Divide Config | Timer divider |

**Enabling APIC** (apic/src/lib.rs):

```rust
pub unsafe fn enable() {
    // Step 1: Enable via MSR
    let mut apic_base: u64;
    core::arch::asm!(
        "rdmsr",
        in("ecx") 0x1B_u32,  // IA32_APIC_BASE MSR
        lateout("eax") apic_base,
        lateout("edx") _
    );
    
    // Set bit 11 (APIC Global Enable)
    if (apic_base & (1 << 11)) == 0 {
        apic_base |= 1 << 11;
        let eax = (apic_base & 0xFFFFFFFF) as u32;
        let edx = ((apic_base >> 32) & 0xFFFFFFFF) as u32;
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0x1B_u32,
            in("eax") eax,
            in("edx") edx
        );
    }
    
    // Step 2: Enable via SVR (Spurious Interrupt Vector Register)
    let svr = lapic_reg(0xF0);
    let val = svr.read_volatile() | 0x100;  // Set bit 8
    svr.write_volatile(val);
}
```

**IA32_APIC_BASE MSR (0x1B)**:

```
Bits    Field
────────────────────────────────────────
0-7     Reserved
8       BSP Flag (1 = Bootstrap Processor)
9-10    Reserved
11      APIC Global Enable
12-35   APIC Base Address (4KB aligned)
36-63   Reserved
```

**SVR Register (0xF0)**:

```
Bits    Field
────────────────────────────────────────
0-7     Spurious Vector Number
8       APIC Software Enable (must be 1)
9       Focus Processor Checking (legacy)
10-11   Reserved
12      EOI Broadcast Suppression
13-31   Reserved
```

### I/O APIC Configuration

**MMIO Base**: 0xFEC00000 (256 bytes)

**Registers**:
| Offset | Register | Access |
|--------|----------|--------|
| 0x00 | IOREGSEL | Write: Select register |
| 0x10 | IOWIN | Read/Write: Data window |

**Indirect Access Model**:
```rust
unsafe fn ioapic_read(reg: u32) -> u32 {
    ioapic_reg(0x00).write_volatile(reg);
    ioapic_reg(0x10).read_volatile()
}

unsafe fn ioapic_write(reg: u32, value: u32) {
    ioapic_reg(0x00).write_volatile(reg);
    ioapic_reg(0x10).write_volatile(value);
}
```

**Redirection Table Registers**:

Each IRQ has a 64-bit redirection entry (two 32-bit registers):

```
Register Index = 0x10 + (IRQ × 2)

Low 32 bits (0x10 + IRQ×2):
┌──────┬──────┬──────┬──────┬──────┬──────┬──────┬──────┐
│ 31-24│ 23-17│ 16   │ 15   │ 14   │ 13   │ 12   │ 11   │
├──────┼──────┼──────┼──────┼──────┼──────┼──────┼──────┤
│ Res  │ Res  │ Mask │Trigger│Remote│Polarity│DelStat│DestMode│
└──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┘
┌──────┬──────────────────────────┐
│ 10-8 │        7-0               │
├──────┼──────────────────────────┤
│DelMode│     Vector              │
└──────┴──────────────────────────┘

High 32 bits (0x10 + IRQ×2 + 1):
┌────────────────────────┬──────────┐
│        31-24           │  23-0    │
├────────────────────────┼──────────┤
│     Destination        │ Reserved │
└────────────────────────┴──────────┘
```

**Field Descriptions**:

| Field | Bits | Values | Description |
|-------|------|--------|-------------|
| Vector | 0-7 | 32-255 | Interrupt vector to deliver |
| Delivery Mode | 8-10 | 000=Fixed | How to deliver (Fixed, Lowest Priority, SMI, NMI, INIT, ExtINT) |
| Destination Mode | 11 | 0=Physical | Physical APIC ID or logical |
| Delivery Status | 12 | RO | 0=Idle, 1=Pending |
| Polarity | 13 | 0=High | 0=Active high, 1=Active low |
| Remote IRR | 14 | RO | Level-triggered interrupt status |
| Trigger Mode | 15 | 0=Edge | 0=Edge triggered, 1=Level triggered |
| Mask | 16 | 0=Enabled | 0=Not masked, 1=Masked |
| Destination | 56-63 | CPU ID | Target APIC ID |

**IRQ Routing** (apic/src/ioapic.rs):

```rust
pub unsafe fn map_irq(irq: u8, vector: u8) {
    let reg = 0x10 + (irq as u32 * 2);
    
    // Low 32 bits: Vector + Fixed delivery + Edge triggered + Not masked
    ioapic_write(reg, vector as u32);
    
    // High 32 bits: Destination = 0 (BSP)
    ioapic_write(reg + 1, 0);
}

pub unsafe fn init_ioapic() {
    map_irq(0, 32);   // Timer (IRQ0) → Vector 32
    map_irq(1, 33);   // Keyboard (IRQ1) → Vector 33
}
```

### LAPIC Timer Configuration

**Purpose**: Periodic interrupts for scheduling and timekeeping.

**Configuration** (apic/src/timer.rs):

```rust
pub const TIMER_VECTOR: u8 = 0x31;              // Vector 49
pub const TIMER_DIVIDE_CONFIG: u32 = 0x3;       // Divide by 16
pub const TIMER_INITIAL_COUNT: u32 = 100_000;   // Period

pub unsafe fn init_hardware() {
    // Divide Configuration Register (0x3E0)
    lapic_reg(0x3E0).write_volatile(TIMER_DIVIDE_CONFIG);
    
    // LVT Timer Register (0x320): Periodic mode
    lapic_reg(0x320).write_volatile((TIMER_VECTOR as u32) | 0x20000);
    
    // Initial Count Register (0x380)
    lapic_reg(0x380).write_volatile(TIMER_INITIAL_COUNT);
}
```

**LVT Timer Register (0x320)**:

```
Bits    Field
────────────────────────────────────────
0-7     Vector
8-10    Reserved (0)
11      Reserved (0)
12      Delivery Status (RO)
13-15   Reserved (0)
16      Mask (0 = Not masked)
17-18   Timer Mode (00=One-shot, 01=Periodic, 10=TSC-Deadline)
19-31   Reserved (0)
```

**Timer Frequency Calculation**:
```
Frequency = Bus Clock / (Divider × Initial Count)

Example:
Bus Clock = 1 GHz
Divider = 16
Initial Count = 100,000

Frequency = 1,000,000,000 / (16 × 100,000)
         = 1,000,000,000 / 1,600,000
         ≈ 625 Hz
         
Period = 1 / 625 ≈ 1.6 ms
```

---

## Interrupt Processing Flow

### Hardware to Handler Execution

```
┌─────────────────────────────────────────────────┐
│ 1. Hardware Event                               │
│    (e.g., key press on keyboard)                │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 2. Device asserts IRQ line                      │
│    (e.g., IRQ1 for keyboard)                    │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 3. I/O APIC receives IRQ                        │
│    - Looks up redirection table entry           │
│    - Determines destination CPU                 │
│    - Determines vector number                   │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 4. I/O APIC sends interrupt message to LAPIC    │
│    via system bus                               │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 5. Local APIC receives interrupt                │
│    - Checks Task Priority Register (TPR)        │
│    - If priority sufficient, signals CPU        │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 6. CPU finishes current instruction             │
│    - Checks INTR pin                            │
│    - Interrupts enabled (IF=1)?                 │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 7. CPU looks up vector in IDT                   │
│    - Reads IDTR for IDT base                    │
│    - Calculates entry address                   │
│    - Loads handler address and segment          │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 8. CPU saves state to stack                     │
│    - SS (if privilege change)                   │
│    - RSP (if privilege change)                  │
│    - RFLAGS                                     │
│    - CS                                         │
│    - RIP                                        │
│    - Error code (if applicable)                 │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 9. CPU clears IF flag (if interrupt gate)       │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 10. CPU jumps to handler                        │
│     Handler prologue (if not naked)             │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 11. Handler executes                            │
│     - Reads device registers                    │
│     - Processes data                            │
│     - Updates kernel state                      │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 12. Handler sends EOI to LAPIC                  │
│     write_volatile(0xFEE000B0, 0)               │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 13. Handler returns (IRETQ)                     │
│     - Pops error code (if present)              │
│     - Pops RIP, CS, RFLAGS                      │
│     - Pops RSP, SS (if privilege change)        │
│     - Restores IF flag from RFLAGS              │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────┐
│ 14. Execution resumes at interrupted code       │
└─────────────────────────────────────────────────┘
```

### Stack Layout During Interrupt

**Same Privilege Level (ring 0 → ring 0)**:

```
        ┌──────────────┐  ← RSP before interrupt
        │   (previous) │
        ├──────────────┤
        │      SS      │  ← Pushed by CPU
        ├──────────────┤
        │     RSP      │  ← Pushed by CPU
        ├──────────────┤
        │   RFLAGS     │  ← Pushed by CPU
        ├──────────────┤
        │      CS      │  ← Pushed by CPU
        ├──────────────┤
        │     RIP      │  ← Pushed by CPU
        ├──────────────┤
        │ Error Code   │  ← Pushed by CPU (if applicable)
        ├──────────────┤  ← RSP in handler
        │   Handler    │
        │    Stack     │
        └──────────────┘
```

**Privilege Change (ring 3 → ring 0)**:

```
Kernel Stack:
        ┌──────────────┐
        │      SS      │  ← User SS
        ├──────────────┤
        │     RSP      │  ← User RSP
        ├──────────────┤
        │   RFLAGS     │
        ├──────────────┤
        │      CS      │  ← User CS
        ├──────────────┤
        │     RIP      │  ← User RIP
        ├──────────────┤
        │ Error Code   │  ← (if applicable)
        ├──────────────┤  ← RSP in handler
        │   Handler    │
        │    Stack     │
        └──────────────┘

User Stack:
        ┌──────────────┐  ← User RSP before interrupt
        │   (previous) │
        └──────────────┘
```

### Handler Function Signature

```rust
extern "x86-interrupt" fn handler_name(stack_frame: InterruptStackFrame)
```

**`extern "x86-interrupt"`**:
- Special calling convention for interrupt handlers
- Compiler generates proper prologue/epilogue
- Handles stack alignment
- Uses `IRETQ` instead of `RET`

**InterruptStackFrame**:
```rust
pub struct InterruptStackFrame {
    pub instruction_pointer: VirtAddr,  // RIP
    pub code_segment: u64,              // CS
    pub cpu_flags: u64,                 // RFLAGS
    pub stack_pointer: VirtAddr,        // RSP
    pub stack_segment: u64,             // SS
}
```

---

## Exception Handlers

### Exception Vector Table

| Vector | Mnemonic | Name | Error Code | Description |
|--------|----------|------|------------|-------------|
| 0 | #DE | Divide Error | No | Division by zero or overflow |
| 1 | #DB | Debug | No | Debug exception |
| 2 | - | NMI | No | Non-maskable interrupt |
| 3 | #BP | Breakpoint | No | INT3 instruction |
| 4 | #OF | Overflow | No | INTO instruction with OF=1 |
| 5 | #BR | BOUND Range Exceeded | No | BOUND instruction |
| 6 | #UD | Invalid Opcode | No | Undefined or reserved opcode |
| 7 | #NM | Device Not Available | No | FPU not available |
| 8 | #DF | Double Fault | Yes (0) | Exception while handling exception |
| 9 | - | Coprocessor Segment Overrun | No | (Legacy, not used) |
| 10 | #TS | Invalid TSS | Yes | Invalid Task State Segment |
| 11 | #NP | Segment Not Present | Yes | Segment not present |
| 12 | #SS | Stack Fault | Yes | Stack segment fault |
| 13 | #GP | General Protection | Yes | General protection violation |
| 14 | #PF | Page Fault | Yes | Page not present or protection violation |
| 15 | - | Reserved | No | (Reserved by Intel) |
| 16 | #MF | x87 FPU Error | No | x87 floating point error |
| 17 | #AC | Alignment Check | Yes (0) | Unaligned memory access (if AM=1, AC=1) |
| 18 | #MC | Machine Check | No | Model-specific fatal hardware error |
| 19 | #XM | SIMD Floating Point | No | SSE/AVX floating point exception |
| 20 | #VE | Virtualization Exception | No | EPT violation (Intel VT-x) |
| 21-31 | - | Reserved | - | Reserved by Intel |

### Divide by Zero Handler

```rust
extern "x86-interrupt" fn divide_by_zero_handler(_stack: InterruptStackFrame) {
    util::panic::oops("Divide by Zero exception");
}
```

**Triggered By**:
- `x / 0` where x is integer
- `i64::MIN / -1` (overflow)

**Action**: Logs error and halts system.

### Page Fault Handler

```rust
extern "x86-interrupt" fn page_fault_handler(
    stack: InterruptStackFrame,
    error_code: PageFaultErrorCode
) {
    // Read CR2 (faulting address)
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
    }
    
    serial_println!("Page fault at instruction pointer: {:#x}", 
                    stack.instruction_pointer.as_u64());
    serial_println!("Page fault address: {:#x}", cr2);
    serial_println!("Error Code: {:?}", error_code);
    
    util::panic::oops("Page fault exception");
}
```

**Error Code Format**:

```
Bit     Name    Description
─────────────────────────────────────────────────────
0       P       0 = Not present, 1 = Protection violation
1       W/R     0 = Read, 1 = Write
2       U/S     0 = Supervisor, 1 = User mode
3       RSVD    1 = Reserved bit set in page table
4       I/D     1 = Instruction fetch
5-31    -       Reserved
```

**CR2 Register**: Contains the linear (virtual) address that caused the page fault.

### Double Fault Handler

```rust
extern "x86-interrupt" fn double_fault_handler(
    _stack: InterruptStackFrame,
    _err: u64
) -> ! {
    serial_println!("Double fault at instruction pointer: {:#x}",
                    _stack.instruction_pointer.as_u64());
    panic!("Double fault exception");
}
```

**Causes**:
- Exception during exception handling
- Stack overflow during exception
- Invalid IDT entry

**Note**: Handler never returns (`-> !`). System is in inconsistent state.

**Future Enhancement**: Use IST (Interrupt Stack Table) to provide separate stack for double fault handler, preventing stack overflow from causing double fault.

---

## Hardware Interrupt Handlers

### Keyboard Interrupt (Vector 33)

```rust
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    
    // Read scancode from keyboard data port
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    
    // Process scancode
    keyboard::handle_scancode(scancode);
    
    // Send EOI to APIC
    unsafe {
        const APIC_EOI: *mut u32 = 0xFEE000B0 as *mut u32;
        APIC_EOI.write_volatile(0);
    }
}
```

**Steps**:
1. Read scancode from port 0x60 (must read or interrupt won't clear)
2. Process scancode (translate to ASCII, output to console)
3. Send EOI to LAPIC (allows next interrupt)

**Critical**: Must read port 0x60 before EOI, or scancode is lost.

### Timer Interrupt (Vector 49)

```rust
extern "x86-interrupt" fn timer_interrupt(_stack_frame: InterruptStackFrame) {
    unsafe {
        TICKS += 1;  // Increment global tick counter
        apic::send_eoi();
    }
}
```

**Purpose**: 
- Timekeeping (system uptime)
- Task scheduling (preemption point)
- Timeout handling

**Frequency**: Currently ~625 Hz (1.6 ms period).

---

## Interrupt Routing

### IRQ to Vector Mapping

```
Hardware IRQ → I/O APIC → Vector → IDT Entry → Handler

Example: Keyboard
IRQ1 → I/O APIC Redirection[1] → Vector 33 → IDT[33] → keyboard_interrupt_handler
```

### I/O APIC Redirection Table

```
IRQ     Device              Vector  Handler
────────────────────────────────────────────────────────
0       PIT Timer           32      (Not actively used)
1       Keyboard            33      keyboard_interrupt_handler
2       Cascade (unused)    -       -
3-15    Available           -       (Not configured)
```

### Local APIC Interrupt Sources

```
Source              Vector      Configuration
─────────────────────────────────────────────────────────
LAPIC Timer         49 (0x31)   LVT Timer Register (0x320)
LINT0 (ExtINT)      -           LVT LINT0 Register (0x350)
LINT1 (NMI)         -           LVT LINT1 Register (0x360)
Error               -           LVT Error Register (0x370)
Performance         -           LVT Performance (0x340)
Thermal             -           LVT Thermal (0x330)
```

### EOI (End of Interrupt)

**Purpose**: Signals to APIC that interrupt has been serviced.

**Location**: LAPIC EOI Register at offset 0xB0

**Address**: 0xFEE000B0 (physical) = 0xFEE000B0 (direct mapped in Serix)

**Operation**:
```rust
pub unsafe fn send_eoi() {
    let eoi = lapic_reg(0xB0);
    eoi.write_volatile(0);  // Write any value (typically 0)
}
```

**Critical**: 
- Must be called at end of every interrupt handler
- Failure to send EOI blocks future interrupts at same/lower priority
- Should be last operation before `IRETQ`

**Exception**: Not needed for CPU exceptions (page fault, divide by zero, etc.)

---

## Performance and Optimization

### Interrupt Latency

**Components**:
```
1. Hardware propagation:           ~100 ns   (Device → I/O APIC → LAPIC)
2. CPU interrupt latency:          ~50 ns    (Current instruction finish)
3. IDT lookup:                     ~10 ns    (TLB hit)
4. State save:                     ~50 ns    (Push registers)
5. Handler dispatch:               ~10 ns    (Jump to handler)
───────────────────────────────────────────────────────────────
Total minimum latency:             ~220 ns

Handler execution:                 Variable  (Depends on handler)
EOI write:                         ~50 ns
State restore:                     ~50 ns
───────────────────────────────────────────────────────────────
Total overhead (excluding handler): ~370 ns
```

**Factors Affecting Latency**:
- Cache misses (IDT, handler code not in cache)
- TLB misses (stack not in TLB)
- Interrupt masking (IF flag clear)
- Task priority (TPR register in LAPIC)
- Pending higher priority interrupts

### Handler Optimization

**Keep Handlers Short**:
```rust
// Good: Minimal work in handler
extern "x86-interrupt" fn interrupt_handler(_stack: InterruptStackFrame) {
    let data = read_device_register();
    BUFFER.push(data);  // Quick operation
    send_eoi();
}

// Bad: Long operation in handler
extern "x86-interrupt" fn interrupt_handler(_stack: InterruptStackFrame) {
    let data = read_device_register();
    process_data(data);     // Long operation
    update_display(data);   // Slow framebuffer access
    send_eoi();
}
```

**Defer Work**: Use "top half / bottom half" pattern:
```rust
// Top half (fast, in handler)
extern "x86-interrupt" fn interrupt_handler(_stack: InterruptStackFrame) {
    let data = read_device_register();
    WORK_QUEUE.push(data);  // Defer processing
    send_eoi();
}

// Bottom half (slow, in main loop or separate task)
fn process_work_queue() {
    while let Some(data) = WORK_QUEUE.pop() {
        process_data(data);
        update_display(data);
    }
}
```

### Interrupt Prioritization

**Hardware Priority**: I/O APIC can route interrupts by priority.

**Software Priority**: LAPIC Task Priority Register (TPR):
```rust
pub unsafe fn set_task_priority(priority: u8) {
    let tpr = lapic_reg(0x80);
    tpr.write_volatile((priority as u32) << 4);
}
```

**Priority Levels**: 0-15 (0 = lowest, 15 = highest)

**Masking**: Interrupts with priority ≤ TPR are masked.

### Spurious Interrupts

**Cause**: LAPIC can generate spurious interrupt if conditions change between interrupt acceptance and delivery.

**Vector**: Typically 255 (0xFF), configured in SVR[0:7].

**Handler**:
```rust
extern "x86-interrupt" fn spurious_interrupt_handler(_stack: InterruptStackFrame) {
    // Do NOT send EOI for spurious interrupts
    // Just return
}
```

**Note**: Spurious interrupts do not require EOI.

---

## Debugging and Troubleshooting

### Common Issues

#### Interrupts Not Firing

**Symptoms**: No keyboard input, timer not ticking.

**Checks**:
1. **IDT loaded?** `init_idt()` called?
2. **Interrupts enabled?** IF flag set (`sti` instruction)?
3. **APIC enabled?** MSR bit 11 and SVR bit 8 set?
4. **I/O APIC configured?** IRQ mapped to vector?
5. **Vector registered?** IDT entry has valid handler?
6. **Device enabled?** Device interrupt not masked?

**Debug**:
```rust
// Check IF flag
let rflags: u64;
unsafe {
    core::arch::asm!("pushfq; pop {}", out(reg) rflags);
}
if rflags & (1 << 9) != 0 {
    serial_println!("Interrupts enabled");
} else {
    serial_println!("Interrupts DISABLED");
}

// Check APIC enabled
let svr = unsafe { lapic_reg(0xF0).read_volatile() };
if svr & 0x100 != 0 {
    serial_println!("APIC enabled");
} else {
    serial_println!("APIC DISABLED");
}
```

#### Interrupt Storm

**Symptoms**: System hangs, 100% CPU usage, no serial output.

**Causes**:
1. Missing EOI (interrupt keeps reasserting)
2. Device not acknowledging (level-triggered interrupt)
3. Infinite loop in handler

**Prevention**:
```rust
extern "x86-interrupt" fn handler(_stack: InterruptStackFrame) {
    // Always send EOI, even on error paths
    let result = process_interrupt();
    
    unsafe { send_eoi(); }  // Guaranteed to execute
    
    if result.is_err() {
        serial_println!("Error in handler");
    }
}
```

#### Double/Triple Fault

**Symptoms**: System reboots or hangs without output.

**Causes**:
1. Stack overflow during exception
2. Invalid IDT entry
3. Page fault in exception handler
4. Exception during double fault

**Debug**:
- Use separate stack for double fault (IST)
- Add debug output at start of exception handlers
- Check page table mappings for handler code and stack

### Diagnostic Tools

#### Interrupt Counters

```rust
static mut IRQ_COUNTS: [AtomicU64; 256] = [const { AtomicU64::new(0) }; 256];

extern "x86-interrupt" fn handler(_stack: InterruptStackFrame) {
    unsafe {
        IRQ_COUNTS[VECTOR].fetch_add(1, Ordering::Relaxed);
    }
    // ... rest of handler
}

pub fn dump_irq_counts() {
    for (vector, count) in IRQ_COUNTS.iter().enumerate() {
        let c = count.load(Ordering::Relaxed);
        if c > 0 {
            serial_println!("Vector {}: {} interrupts", vector, c);
        }
    }
}
```

#### APIC Register Dump

```rust
pub unsafe fn dump_apic_state() {
    serial_println!("=== APIC State ===");
    serial_println!("ID:      {:#010x}", lapic_reg(0x020).read_volatile());
    serial_println!("Version: {:#010x}", lapic_reg(0x030).read_volatile());
    serial_println!("TPR:     {:#010x}", lapic_reg(0x080).read_volatile());
    serial_println!("SVR:     {:#010x}", lapic_reg(0x0F0).read_volatile());
    serial_println!("LVT Timer: {:#010x}", lapic_reg(0x320).read_volatile());
    serial_println!("Timer Init: {:#010x}", lapic_reg(0x380).read_volatile());
    serial_println!("Timer Cur:  {:#010x}", lapic_reg(0x390).read_volatile());
}
```

#### I/O APIC Redirection Dump

```rust
pub unsafe fn dump_ioapic_redirects() {
    serial_println!("=== I/O APIC Redirection Table ===");
    for irq in 0..24 {
        let reg = 0x10 + (irq * 2);
        let low = ioapic_read(reg);
        let high = ioapic_read(reg + 1);
        
        let vector = low & 0xFF;
        let masked = (low >> 16) & 1;
        let dest = high >> 24;
        
        serial_println!("IRQ {}: Vector={} Mask={} Dest={}",
                        irq, vector, masked, dest);
    }
}
```

---

## Appendix

### Interrupt Vector Summary

```
Vector  Type        Handler                          Module
────────────────────────────────────────────────────────────────────
0       Exception   divide_by_zero_handler           idt/src/lib.rs
8       Exception   double_fault_handler             idt/src/lib.rs
14      Exception   page_fault_handler               idt/src/lib.rs
33      Hardware    keyboard_interrupt_handler       idt/src/lib.rs
49      Hardware    timer_interrupt                  apic/src/timer.rs
```

### Register Addresses

```
Local APIC:     0xFEE00000 (MMIO, 4 KB)
I/O APIC:       0xFEC00000 (MMIO, 256 bytes)
PIC1 Command:   0x20 (Port I/O)
PIC1 Data:      0x21 (Port I/O)
PIC2 Command:   0xA0 (Port I/O)
PIC2 Data:      0xA1 (Port I/O)
Keyboard Data:  0x60 (Port I/O)
```

### Key Constants

```rust
// apic/src/lib.rs
const APIC_BASE: u64 = 0xFEE00000;

// apic/src/ioapic.rs
const IOAPIC_BASE: u64 = 0xFEC00000;

// apic/src/timer.rs
pub const TIMER_VECTOR: u8 = 0x31;
pub const TIMER_DIVIDE_CONFIG: u32 = 0x3;
pub const TIMER_INITIAL_COUNT: u32 = 100_000;
```

---

**End of Document**
