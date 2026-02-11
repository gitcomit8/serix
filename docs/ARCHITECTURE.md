# Serix Kernel Architecture

## Overview

Serix is a capability-based microkernel-inspired operating system written in
Rust for the x86_64 architecture. This document describes the architectural
design, subsystem organization, and implementation philosophy of the kernel.

## Current Status (v0.0.5)

Serix v0.0.5 is a kernel with these working features:

- x86_64 long mode, higher-half kernel
- APIC interrupt controller (Local APIC + I/O APIC)
- Timer interrupts (~625 Hz) and keyboard input
- Serial console and framebuffer graphics
- Memory management (paging, heap, frame allocator)
- VFS with ramdisk and ELF loader
- Basic syscalls and userspace init execution

See individual subsystem documentation for detailed information.

## Subsystem Documentation

For detailed information about each subsystem, refer to:

- **Boot Process**: [BOOT_PROCESS.md](BOOT_PROCESS.md)
- **Memory Management**: [MEMORY_LAYOUT.md](MEMORY_LAYOUT.md), [memory module](../memory/README.md)
- **Interrupt Handling**: [INTERRUPT_HANDLING.md](INTERRUPT_HANDLING.md), [APIC module](../apic/README.md), [IDT module](../idt/README.md)
- **Hardware Abstraction**: [HAL_API.md](HAL_API.md), [HAL module](../hal/README.md)
- **Graphics System**: [GRAPHICS_API.md](GRAPHICS_API.md), [graphics module](../graphics/README.md)
- **Syscalls & Kernel API**: [KERNEL_API.md](KERNEL_API.md), [kernel module](../kernel/README.md)
- **Task Management**: [task module](../task/README.md)
- **Device Drivers**: [keyboard module](../keyboard/README.md)
- **Utilities**: [util module](../util/README.md)

## Development

- **Roadmap**: See [ROADMAP.md](ROADMAP.md) for planned features and milestones
- **Contributing**: See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines
