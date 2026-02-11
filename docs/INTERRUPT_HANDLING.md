=======================================

# Interrupt Handling in Serix (x86_64)

:Last Updated: 2025-10-13

## Overview

Serix implements a modern interrupt handling infrastructure based on the x86_64
Interrupt Descriptor Table (IDT) and Advanced Programmable Interrupt Controller
(APIC). This document describes the complete interrupt handling mechanism from
hardware signal to software handler execution.

## Current Status (v0.0.5)

Working features

```

```

- APIC fully enabled (Local APIC + I/O APIC)
- Legacy PIC disabled and masked
- IDT loaded with 256 entries (exceptions + interrupts)
- Timer interrupts operational (vector 49, ~625 Hz)
- Keyboard interrupts operational (vector 33, PS/2)
- Exception handlers registered (divide-by-zero, page fault, double fault)
- EOI (End of Interrupt) correctly sent to LAPIC

## Key Components

===========  ===================================  =============================
Component    Purpose                              Location
===========  ===================================  =============================
IDT          Maps interrupt vectors to handlers   IDTR register
Local APIC   Per-CPU interrupt controller         MMIO at 0xFEE00000
I/O APIC     Routes hardware IRQs to CPUs         MMIO at 0xFEC00000
Legacy PIC   Disabled, not used                   Ports 0x20/0x21, 0xA0/0xA1
===========  ===================================  =============================

## Interrupt Vector Allocation

Current vector assignments in v0.0.5

```


```

## Interrupt Architecture

## x86_64 Interrupt Model

Hardware interrupt flow from device to handler execution

```

┌───────────────┐
│   APIC/CPU    │
│  Determines   │
│    Vector     │
└───────┬───────┘
┌───────────────┐
│  CPU looks up │
│  vector in    │
│     IDT       │
└───────┬───────┘
┌───────────────┐
│  CPU pushes   │
│  state to     │
│    stack      │
└───────┬───────┘
┌───────────────┐
│  Jump to      │
│  handler      │
│  address      │
└───────┬───────┘
┌───────────────┐
│  Handler      │
│  executes     │
└───────┬───────┘
┌───────────────┐
│  Send EOI     │
│  to APIC      │
└───────┬───────┘
┌───────────────┐
│  IRETQ back   │
│  to code      │
└───────────────┘

```

## Interrupt vs Exception

=========  ===================================  ================================
Aspect     Exception                            Interrupt
=========  ===================================  ================================
Source     CPU (synchronous)                    Hardware/Software (asynchronous)
Timing     Precise (at instruction boundary)    Approximate (between instructions)
Vector     0-31 (fixed)                         32-255 (configurable)
Error Code Some push error code                 None (interrupt number only)
Examples   Page fault, divide by zero           Timer, keyboard, disk I/O
=========  ===================================  ================================

## IDT Structure and Setup

## Interrupt Descriptor Table (IDT)

**Size**: 256 entries × 16 bytes = 4096 bytes (1 page)

**Location**: Kernel memory, address loaded into IDTR register

Entry Format

```~~~~~~~~~

16 bytes per entry

```

```
Gate Types
```~~~~~~~

**Interrupt Gate (0xE)**:
  * Automatically disables interrupts (clears IF flag)
  * Used for most interrupt handlers
  * Prevents reentrant interrupts during handler execution

**Trap Gate (0xF)**:
  * Does not disable interrupts (IF flag unchanged)
  * Used for debugging and system calls
  * Allows interrupts during handler execution

**Serix Policy**: Use interrupt gates for all handlers (safety first).


## IDT Initialization

Implementation in idt/src/lib.rs

```

```
IDTR Register
```~~~~~~~~~~

**Format** (10 bytes)

```

```

**Fields**:

- **Limit**: Size of IDT - 1 (typically 4095 for 256 entries)
- **Base**: Linear address of IDT

**Loading IDTR**

```

```


## APIC Configuration


## Legacy PIC Disable

Why Disable?

```~~~~~~~~~

  * APIC is superior (better multiprocessor support, more IRQs)
  * PIC conflicts with APIC if both active
  * PIC defaults to vectors 0-15 (overlaps CPU exceptions)

Disable Procedure
```~~~~~~~~~~~~~~

Implementation in apic/src/lib.rs

```

```

PIC Ports

```

```


## Local APIC Enable

**MMIO Base**: 0xFEE00000 (4 KB region)

APIC Registers

```~~~~~~~~~~~

======  ==========  ================================
Offset  Register    Purpose
======  ==========  ================================
0x020   ID          Local APIC ID (CPU identifier)
0x030   Version     APIC version and max LVT entries
0x080   TPR         Task Priority Register
0x0B0   EOI         End of Interrupt
0x0F0   SVR         Spurious Interrupt Vector Register
0x320   LVT Timer   Timer interrupt configuration
0x350   LVT LINT0   External interrupt 0
0x360   LVT LINT1   External interrupt 1 (NMI)
0x370   LVT Error   Error interrupt
0x380   Timer Init  Timer countdown value
0x390   Timer Cur   Current timer value (read-only)
0x3E0   Timer Div   Timer divider
======  ==========  ================================

Enabling APIC
```~~~~~~~~~~

Implementation in apic/src/lib.rs

```

```
IA32_APIC_BASE MSR (0x1B)
```~~~~~~~~~~~~~~~~~~~~~~

```

```

SVR Register (0xF0)

```~~~~~~~~~~~~~~~~


```

```

## I/O APIC Configuration

**MMIO Base**: 0xFEC00000 (256 bytes)

I/O APIC Registers
```~~~~~~~~~~~~~~~

======  ========  =========================
Offset  Register  Access
======  ========  =========================
0x00    IOREGSEL  Write: Select register
0x10    IOWIN     Read/Write: Data window
======  ========  =========================

Indirect Access Model
```~~~~~~~~~~~~~~~~~~

```

```

Redirection Table Registers

```~~~~~~~~~~~~~~~~~~~~~~~~~

Each IRQ has a 64-bit redirection entry (two 32-bit registers)

```

```
Field Descriptions:

================  ======  ================  ======================================
Field             Bits    Values            Description
================  ======  ================  ======================================
Vector            0-7     32-255            Interrupt vector to deliver
Delivery Mode     8-10    000=Fixed         How to deliver (Fixed, etc.)
Destination Mode  11      0=Physical        Physical APIC ID or logical
Delivery Status   12      RO                0=Idle, 1=Pending
Polarity          13      0=High            0=Active high, 1=Active low
Remote IRR        14      RO                Level-triggered interrupt status
Trigger Mode      15      0=Edge            0=Edge triggered, 1=Level triggered
Mask              16      0=Enabled         0=Not masked, 1=Masked
Destination       56-63   CPU ID            Target APIC ID
================  ======  ================  ======================================

IRQ Routing
```~~~~~~~~

Implementation in apic/src/ioapic.rs

```

```

## LAPIC Timer Configuration

Purpose
```~~~~

Periodic interrupts for scheduling and timekeeping.

Configuration
```~~~~~~~~~~

Implementation in apic/src/timer.rs

```

```
LVT Timer Register (0x320)
```~~~~~~~~~~~~~~~~~~~~~~~


```

```
Timer Frequency Calculation
```~~~~~~~~~~~~~~~~~~~~~~~~~

```

```


## Interrupt Processing Flow


## Hardware to Handler Execution

Complete flow from hardware event to handler completion

```

```


## Stack Layout During Interrupt

Same Privilege Level (ring 0 → ring 0)

```~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~


```

```
Privilege Change (ring 3 → ring 0)
```~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Kernel Stack

```

```
User Stack

```

```

## Handler Function Signature

Rust calling convention for interrupt handlers

```

```
**extern "x86-interrupt"**:
  * Special calling convention for interrupt handlers
  * Compiler generates proper prologue/epilogue
  * Handles stack alignment
  * Uses IRETQ instead of RET

**InterruptStackFrame**

```

```

## Exception Handlers


## Exception Vector Table

Complete x86_64 exception vector table:

======  ========  =====================  ==========  ====================================
Vector  Mnemonic  Name                   Error Code  Description
======  ========  =====================  ==========  ====================================
0       #DE       Divide Error           No          Division by zero or overflow
1       #DB       Debug                  No          Debug exception
2       \-        NMI                    No          Non-maskable interrupt
3       #BP       Breakpoint             No          INT3 instruction
4       #OF       Overflow               No          INTO instruction with OF=1
5       #BR       BOUND Range Exceeded   No          BOUND instruction
6       #UD       Invalid Opcode         No          Undefined or reserved opcode
7       #NM       Device Not Available   No          FPU not available
8       #DF       Double Fault           Yes (0)     Exception while handling exception
9       \-        Coprocessor Overrun    No          (Legacy, not used)
10      #TS       Invalid TSS            Yes         Invalid Task State Segment
11      #NP       Segment Not Present    Yes         Segment not present
12      #SS       Stack Fault            Yes         Stack segment fault
13      #GP       General Protection     Yes         General protection violation
14      #PF       Page Fault             Yes         Page not present or protection violation
15      \-        Reserved               No          (Reserved by Intel)
16      #MF       x87 FPU Error          No          x87 floating point error
17      #AC       Alignment Check        Yes (0)     Unaligned memory access (if AM=1, AC=1)
18      #MC       Machine Check          No          Model-specific fatal hardware error
19      #XM       SIMD Floating Point    No          SSE/AVX floating point exception
20      #VE       Virtualization         No          EPT violation (Intel VT-x)
21-31   \-        Reserved               \-          Reserved by Intel
======  ========  =====================  ==========  ====================================


## Divide by Zero Handler

Implementation in idt/src/lib.rs

```

```
**Triggered By**:
  * ``x / 0`` where x is integer
  * ``i64::MIN / -1`` (overflow)

**Action**: Logs error and halts system.


## Page Fault Handler

Implementation in idt/src/lib.rs

```

```
Error Code Format
```~~~~~~~~~~~~~~


```

```
**CR2 Register**: Contains the linear (virtual) address that caused the page fault.


## Double Fault Handler

Implementation in idt/src/lib.rs

```

```
**Causes**:
  * Exception during exception handling
  * Stack overflow during exception
  * Invalid IDT entry

**Note**: Handler never returns (``-> !``). System is in inconsistent state.

**Future Enhancement**: Use IST (Interrupt Stack Table) to provide separate
stack for double fault handler, preventing stack overflow from causing double
fault.


## Hardware Interrupt Handlers


## Keyboard Interrupt (Vector 33)

Status: **WORKING IN v0.0.5**

Implementation in idt/src/lib.rs

```

```
**Steps**:
  1. Read scancode from port 0x60 (must read or interrupt won't clear)
  2. Process scancode (translate to ASCII, output to console)
  3. Send EOI to LAPIC (allows next interrupt)

**Critical**: Must read port 0x60 before EOI, or scancode is lost.


## Timer Interrupt (Vector 49)

Status: **WORKING IN v0.0.5** (~625 Hz)

Implementation in apic/src/timer.rs

```

```
**Purpose**: 
  * Timekeeping (system uptime)
  * Task scheduling (preemption point)
  * Timeout handling

**Frequency**: Currently ~625 Hz (1.6 ms period).


## Interrupt Routing


## IRQ to Vector Mapping

Complete routing from hardware IRQ to handler

```

```

## I/O APIC Redirection Table

Current configuration in v0.0.5

```

```

## Local APIC Interrupt Sources

LAPIC-generated interrupts

```

```

## EOI (End of Interrupt)

Purpose
```~~~~

Signals to APIC that interrupt has been serviced.

**Location**: LAPIC EOI Register at offset 0xB0

**Address**: 0xFEE000B0 (physical) = 0xFEE000B0 (direct mapped in Serix)

Operation
```~~~~~~

Implementation in apic/src/lib.rs

```

```
Critical Requirements
```~~~~~~~~~~~~~~~~~~

  * Must be called at end of every interrupt handler
  * Failure to send EOI blocks future interrupts at same/lower priority
  * Should be last operation before IRETQ

**Exception**: Not needed for CPU exceptions (page fault, divide by zero, etc.)


## Performance and Optimization


## Interrupt Latency

Latency breakdown

```

```
Factors Affecting Latency
```~~~~~~~~~~~~~~~~~~~~~~~

  * Cache misses (IDT, handler code not in cache)
  * TLB misses (stack not in TLB)
  * Interrupt masking (IF flag clear)
  * Task priority (TPR register in LAPIC)
  * Pending higher priority interrupts


## Handler Optimization

Keep Handlers Short
```~~~~~~~~~~~~~~~~

Good: Minimal work in handler

```

```
Bad: Long operation in handler

```

```
Defer Work
```~~~~~~~

Use "top half / bottom half" pattern

```

```

## Interrupt Prioritization

Hardware Priority
```~~~~~~~~~~~~~~

I/O APIC can route interrupts by priority.

Software Priority
```~~~~~~~~~~~~~~

LAPIC Task Priority Register (TPR)

```

```
**Priority Levels**: 0-15 (0 = lowest, 15 = highest)

**Masking**: Interrupts with priority ≤ TPR are masked.


## Spurious Interrupts

**Cause**: LAPIC can generate spurious interrupt if conditions change between
interrupt acceptance and delivery.

**Vector**: Typically 255 (0xFF), configured in SVR[0:7].

Handler Implementation
```~~~~~~~~~~~~~~~~~~~


```

```
**Note**: Spurious interrupts do not require EOI.


## Debugging and Troubleshooting


## Common Issues

Interrupts Not Firing
```~~~~~~~~~~~~~~~~~~~

**Symptoms**: No keyboard input, timer not ticking.

**Checks**:
  1. **IDT loaded?** init_idt() called?
  2. **Interrupts enabled?** IF flag set (sti instruction)?
  3. **APIC enabled?** MSR bit 11 and SVR bit 8 set?
  4. **I/O APIC configured?** IRQ mapped to vector?
  5. **Vector registered?** IDT entry has valid handler?
  6. **Device enabled?** Device interrupt not masked?

Debug Code
++++++++++


```

```
Interrupt Storm
```~~~~~~~~~~~~

**Symptoms**: System hangs, 100% CPU usage, no serial output.

**Causes**:
  1. Missing EOI (interrupt keeps reasserting)
  2. Device not acknowledging (level-triggered interrupt)
  3. Infinite loop in handler

Prevention
++++++++++


```

```
Double/Triple Fault
```~~~~~~~~~~~~~~~~

**Symptoms**: System reboots or hangs without output.

**Causes**:
  1. Stack overflow during exception
  2. Invalid IDT entry
  3. Page fault in exception handler
  4. Exception during double fault

**Debug**:
  * Use separate stack for double fault (IST)
  * Add debug output at start of exception handlers
  * Check page table mappings for handler code and stack


## Diagnostic Tools

Interrupt Counters
```~~~~~~~~~~~~~~~


```

```
APIC Register Dump
```~~~~~~~~~~~~~~~


```

```
I/O APIC Redirection Dump
```~~~~~~~~~~~~~~~~~~~~~~~


```

```

## Appendix


## Interrupt Vector Summary

Active vectors in v0.0.5

```

```

## Register Addresses

MMIO and I/O port addresses

```

```

## Key Constants

From source code

```

```

## See Also

  * Intel® 64 and IA-32 Architectures Software Developer's Manual, Volume 3
  * AMD64 Architecture Programmer's Manual, Volume 2
  * OSDev Wiki: https://wiki.osdev.org/Interrupts
  * OSDev Wiki: https://wiki.osdev.org/APIC

## See Also

- **[Boot Process](BOOT_PROCESS.md)** - IDT and APIC initialization sequence
- **[Architecture Overview](ARCHITECTURE.md)** - System design overview
- **[APIC Module](../apic/README.md)** - APIC interrupt controller implementation
- **[IDT Module](../idt/README.md)** - Interrupt Descriptor Table implementation
- **[Keyboard Module](../keyboard/README.md)** - PS/2 keyboard interrupt handler
