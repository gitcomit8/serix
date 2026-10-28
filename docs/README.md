# Serix Kernel Documentation

This directory contains comprehensive technical documentation for the Serix hybrid-kernel operating system.

## Core Documentation

### System Architecture

- **[Architecture Overview](ARCHITECTURE.md)** - High-level system design, ring model, subsystem organization, and crate map
- **[Boot Process](BOOT_PROCESS.md)** - Detailed boot sequence from firmware to userspace
- **[Memory Layout](MEMORY_LAYOUT.md)** - Virtual and physical memory organization
- **[Roadmap](ROADMAP.md)** - Development roadmap, milestones, and implementation status

### Hardware & Interrupts

- **[Interrupt Handling](INTERRUPT_HANDLING.md)** - IDT, APIC, and interrupt flow
- **[HAL API](HAL_API.md)** - Hardware abstraction layer interface

### APIs & Interfaces

- **[Kernel API](KERNEL_API.md)** - Syscall interface, argument conventions, and usage
- **[Graphics API](GRAPHICS_API.md)** - Framebuffer operations and console

### Development

- **[Contributing](../CONTRIBUTING.md)** - Code style, build procedures, and PR process

## Module Documentation

Each subsystem has its own README with implementation details:

| Module | Description | Documentation |
| ------ | ----------- | ------------- |
| **kernel** | Entry point, syscalls, GDT, kshell, LES bridge | [kernel/README.md](../kernel/README.md) |
| **memory** | Page tables, heap, frame allocator, kstack, VMA | [memory/README.md](../memory/README.md) |
| **hal** | Hardware abstraction (serial, CPU, I/O ports) | [hal/README.md](../hal/README.md) |
| **apic** | APIC interrupt controller (LAPIC, I/O APIC, timer) | [apic/README.md](../apic/README.md) |
| **idt** | Interrupt descriptor table | [idt/README.md](../idt/README.md) |
| **graphics** | Framebuffer console, drawing primitives | [graphics/README.md](../graphics/README.md) |
| **task** | Async task executor, scheduler, WFQ | [task/README.md](../task/README.md) |
| **capability** | Capability-based security system | [capability/](../capability/README.md) |
| **ipc** | Port-based IPC, message passing | [ipc/README.md](../ipc/README.md) |
| **vfs** | Virtual filesystem, mount table | [vfs/README.md](../vfs/README.md) |
| **fs** | Filesystem drivers (FAT32, ext2, ext4) | [fs/README.md](../fs/README.md) |
| **drivers** | Device drivers (VirtIO, PCI) | [drivers/README.md](../drivers/README.md) |
| **keyboard** | PS/2 keyboard driver | [keyboard/README.md](../keyboard/README.md) |
| **util** | Utility functions, panic handler | [util/README.md](../util/README.md) |

## Quick Navigation

### Getting Started

1. New to Serix? Start with the [main README](../README.md)
2. Want to contribute? Read [CONTRIBUTING.md](../CONTRIBUTING.md)
3. Understand the architecture: [ARCHITECTURE.md](ARCHITECTURE.md)
4. Understand the boot process: [BOOT_PROCESS.md](BOOT_PROCESS.md)
5. Learn the memory layout: [MEMORY_LAYOUT.md](MEMORY_LAYOUT.md)

### Developer References

- Building and testing: See [README.md - Building](../README.md#building)
- Code style guidelines: See [CONTRIBUTING.md - Code Style](../CONTRIBUTING.md#code-style-guidelines)
- Syscall reference: [KERNEL_API.md](KERNEL_API.md)
- Hardware interaction: [HAL_API.md](HAL_API.md)

### Architecture Deep Dives

- System architecture: [ARCHITECTURE.md](ARCHITECTURE.md)
- How interrupts work: [INTERRUPT_HANDLING.md](INTERRUPT_HANDLING.md)
- Memory management: [MEMORY_LAYOUT.md](MEMORY_LAYOUT.md)
- Graphics system: [GRAPHICS_API.md](GRAPHICS_API.md)

## External Resources

- **Limine Bootloader Protocol**: [GitHub - Limine Protocol](https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md)
- **OSDev Wiki**: [https://wiki.osdev.org](https://wiki.osdev.org)
- **Intel x86_64 Manual**: [Intel® 64 and IA-32 Architectures Software Developer Manuals](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- **AMD64 Architecture**: [AMD64 Architecture Programmer's Manual](https://www.amd.com/en/support/tech-docs)

## Documentation Standards

All documentation in this repository follows:

- **GitHub Flavored Markdown** syntax
- **ATX-style headings** (using `#` symbols)
- **Fenced code blocks** with language identifiers
- **Consistent linking** for cross-references

For detailed documentation guidelines, see [CONTRIBUTING.md](../CONTRIBUTING.md).

## License

All documentation is licensed under GNU General Public License v3.0. See [LICENSE](../LICENSE) file.
