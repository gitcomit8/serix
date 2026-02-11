================================================================================

# Serix Hardware Abstraction Layer (HAL) Documentation

:Last Updated: 2025-01-XX
:Module Path: hal/src/

.. Contents:

   1. Introduction
   2. Hardware Initialization
   3. Serial Console Driver
   4. CPU Control Interface  
   5. I/O Port Operations
   6. CPU Topology Detection
   7. Debugging and Tracing
   8. Future Work

## 1. Introduction

The Serix HAL provides low-level interfaces to x86_64 hardware, isolating
platform-specific code from kernel subsystems. This layer exposes safe Rust
abstractions over CPU instructions, legacy I/O ports, and serial communication
devices while maintaining zero-cost performance characteristics.

## 1.1 Design Principles

- Zero-Cost Abstractions - Inline assembly with no runtime overhead
- Type-Safe Hardware Access - Rust's type system prevents hardware bugs  
- Minimal Unsafe Surface - Unsafe operations clearly marked and isolated
- Direct Hardware Control - No buffering or indirection layers

## 1.2 Current Features (v0.0.5)

Serial Console (WORKING)

```

```

- COM1 UART 16550 driver fully operational
- Debug output via serial_println! macro
- 115200 baud, 8N1 configuration
- Thread-safe global singleton with spinlock protection

CPU Control (WORKING)

```

```

- Interrupt enable/disable (CLI/STI instructions)
- Halt instruction (HLT) for idle loops
- Basic CPU feature detection via CPUID

I/O Port Access (WORKING)

```

```

- Low-level inb/outb primitives for legacy devices
- 16-bit port address space (0x0000-0xFFFF)

CPU Topology (BASIC)

```

```

- Single-CPU detection
- Placeholder for multi-core enumeration

## 1.3 Module Organization

```


```

Dependencies

```


```

## 1. Hardware Initialization

## 2.1 Boot Sequence

The HAL is initialized early during kernel boot, immediately after the Limine
bootloader transfers control to the kernel entry point (_start). The following
sequence must be strictly observed:

1. Initialize serial console (hal::init_serial)
2. Disable legacy PIC, enable APIC
3. Load IDT with exception/interrupt handlers  
4. Enable interrupts (STI instruction)
5. Other subsystems may now use serial_println!

Example initialization code

```






```

.. warning

```

```

## 2.2 Early Boot Constraints

During early boot (before heap initialization), the following restrictions apply:

- No dynamic allocations (Vec, Box, String, format!)
- No serial_println! before init_serial()
- Interrupts disabled until IDT loaded
- Stack is limited (typically 64 KB from bootloader)

The serial console is the ONLY output mechanism available during early boot.
Framebuffer initialization happens much later, after memory management is ready.

## 2.3 Asciinema Demo

```


### Interrupt Control Path

```

disable_interrupts()
    → cpu::disable_interrupts()
    → x86_64::instructions::interrupts::disable()
    → asm!("cli")
    → CPU clears IF flag in RFLAGS
    → Interrupts masked

```

## ## 3. Serial Console Driver

The serial console is the primary debug interface during kernel boot and runtime.
It provides reliable output before framebuffer initialization and persists even
if graphics fail. All kernel boot messages, panics, and debug output route
through COM1.


## 3.1 Hardware: UART 16550

The 16550 Universal Asynchronous Receiver/Transmitter (UART) is a legacy serial
controller present in all x86 systems (real hardware and QEMU/VirtualBox).

Base Address (COM1)

```

```
Register Map (DLAB=0)

```

```
When DLAB=1 (Line Control Register bit 7 set)

```

```

## 3.2 Initialization Sequence

The serial port must be configured before any output is possible. This is done
in hal::serial::SerialPort::new().

Initialization Steps

```

```
Code example from hal/src/serial.rs

```

```

## 3.3 Transmitting Data

Transmission is polled (no interrupts used). Each byte is sent by:

1. Wait for transmitter to be ready (poll LSR bit 5)
2. Write byte to data register (offset +0)
3. UART serializes byte and sends over TX line

Line Status Register (LSR) Bit 5 - THRE

```

```
Typical transmission time

```

```
Code

```

```
For strings

```

```

## 3.4 Thread-Safe Global Serial Port

The kernel provides a global serial port protected by a spinlock. This allows
safe concurrent access from interrupt handlers and kernel threads.

Global singleton

```

```
Thread-safe print function

```

```
Convenience macros

```

```
Usage

```

```

## 3.5 Reading Serial Input (Future)

Currently, the serial driver is transmit-only. Receive functionality is planned
for v0.1.0 and will use interrupts (IRQ 4 via APIC).

Planned RX interrupt handler

```

```

## 3.6 Debugging Serial Output

QEMU Configuration

```

```
VirtualBox Configuration

```

```
Real Hardware

```

```
3.7 Port I/O Implementation Details
```~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The serial driver uses hal::io::inb and hal::io::outb for register access.
These are thin wrappers over x86 IN/OUT instructions.

Assembly for outb (write to I/O port)

```

```
Assembly for inb (read from I/O port)

```

```
Why unsafe

```

```
I/O port address space

```

```
Performance characteristics

```

```

## 4. CPU Control Interface

Module: hal::cpu

Provides safe wrappers over x86_64 CPU control instructions (HLT, CLI, STI) and
basic CPU feature detection via CPUID.


## 4.1 Halt Instruction

Function

```

```
Halts the CPU until the next interrupt arrives. The processor enters a low-power
state (C1) and will wake on any interrupt, including timer, keyboard, or NMI.

Assembly

```

```
Behavior

```

```
Usage in idle loop

```

```
.. warning

```

```

## 4.2 Interrupt Control

Enable Interrupts (STI)

```

```
Disable Interrupts (CLI)

```

```
Critical Section Pattern

```

```
Manual control (less safe)

```

```
Interrupt State Query

```

```
Maximum Interrupt-Disabled Duration

```

```

## 4.3 CPU Identification (CPUID)

The CPUID instruction provides CPU vendor, model, and feature information.

Basic usage from x86_64 crate

```

```
Feature flags (CPUID leaf 1, EDX)

```

```
HAL CPU topology module (hal/src/topology.rs) uses CPUID to enumerate cores,
but this is currently a stub returning 1 CPU.



## 5. I/O Port Operations

Module: hal::io

Low-level x86 I/O port access via IN and OUT instructions. The x86 architecture
has a separate 16-bit I/O address space (65,536 ports) distinct from memory.


## 5.1 Port Address Space

Address Range

```

```
Common Port Ranges

```

```

## 5.2 Output to Port (outb)

Function

```

```
Writes an 8-bit value to the specified I/O port.

Parameters

```

```
Assembly

```

```
Example - Write to serial port

```

```
Example - Disable PIC

```

```
Safety Requirements

```

```

## 5.3 Input from Port (inb)

Function

```

```
Reads an 8-bit value from the specified I/O port.

Parameters

```

```
Returns

```

```
Assembly

```

```
Example - Read serial port status

```

```
Example - Poll keyboard controller

```

```
Side Effects

```

```

## 5.4 Performance Characteristics

I/O port operations are significantly slower than memory access

```

```
Why so slow

```

```
Serialization

```

```
Use MMIO for performance-critical devices

```

```

## 6. CPU Topology Detection

Module: hal::topology

Detects number of CPUs, cores, and threads in the system using CPUID and ACPI
tables. Currently this module is a stub returning 1 CPU.


## 6.1 Current Implementation (v0.0.5)

Function

```

```
Returns

```

```
Planned for v0.1.0

```

```

## 6.2 Multi-Processor Detection (Future)

ACPI MADT provides list of Local APICs

```

```
CPUID Topology Enumeration

```

```
Example code (planned)

```

```

## 7. Debugging and Tracing


## 7.1 Serial Console Debugging

The serial console is the primary debugging interface. It works in all scenarios:
- Early boot (before heap, framebuffer)
- Kernel panics (when graphics may be corrupted)
- Interrupt handlers (when framebuffer is unsafe)
- Real hardware (via null modem cable)

Logging conventions

```

```
Checkpoint logging during boot

```

```
This helps isolate hangs and triple faults.


## 7.2 Debug Output Configuration

QEMU

```

```
VirtualBox

```

```
Real Hardware

```

```

## 7.3 Common Debugging Scenarios

Kernel hangs during boot

```

```
Triple fault

```

```
Interrupt handler debugging

```

```

## 7.4 Asciinema Recordings

This documentation references asciinema recordings demonstrating HAL operation:

recordings/serial-console-demo.cast

```

```
recordings/hal-init-sequence.cast

```

```
To record your own

```

```

## 8. Future Work


## 8.1 Planned for v0.1.0

Serial RX Support

```

```
CPU Topology

```

```
MSR Access

```

```

## 8.2 Planned for v0.2.0

PCI Configuration Space

```

```
DMA Support

```

```
Performance Monitoring

```

```

## 8.3 Planned for v1.0.0

ACPI Integration

```

```
SMP Bootstrapping

```

```
Hardware Watchdog

```

```
================================================================================

## End of HAL Documentation

## See Also

- **[Boot Process](BOOT_PROCESS.md)** - HAL initialization during boot
- **[Architecture Overview](ARCHITECTURE.md)** - HAL in system architecture
- **[HAL Module](../hal/README.md)** - Hardware abstraction layer implementation
- **[Interrupt Handling](INTERRUPT_HANDLING.md)** - Hardware interrupt management
