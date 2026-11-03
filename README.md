# Serix Kernel

Serix is a hybrid-kernel x86_64 operating system written in `#![no_std]` Rust with capability-based security. The kernel boots via the Limine bootloader and provides a minimal userspace execution environment with VFS, IPC, and preemptive scheduling.

## Status

Current release: v0.0.8

The kernel successfully boots to a graphical console, initializes core
subsystems (APIC, IDT, heap, VFS), spawns the Ring-3 `ext4d` daemon and
kernel shell tasks, and responds to keyboard/timer interrupts.

[Screenshot placeholder: Boot screen with blue framebuffer and memory map visualization]
Alt text: "Serix kernel boot screen showing blue framebuffer with colored memory map bars at bottom and white text displaying boot messages"

[Asciinema recording placeholder: Complete boot sequence from QEMU start to init execution]
Alt text: "Terminal recording showing QEMU boot of Serix kernel with serial console output displaying all initialization checkpoints from serial init through init binary execution, approximately 30 seconds"

## Features

- x86_64 long mode higher-half kernel with HHDM, booting via Limine v10.x (BIOS + UEFI)
- Capability-based security: per-task cspace, capability-gated syscalls, revocation cascading, delegation, audit ring
- Physical memory management with boot-time frame allocator (`StaticBootFrameAllocator`, allocation-only)
- Virtual memory with 4-level paging (PML4), SLUB allocator for large objects, 1 MiB per-task kernel stacks with guard pages
- Heap allocator (1 MiB default kernel heap)
- SMP with Limine MP: AP bootstrap via callback mechanism, per-CPU run queues, per-CPU kernel stacks
- APIC interrupt controller (Local APIC + I/O APIC, legacy PIC disabled)
- Interrupt descriptor table with exception handlers
- LAPIC timer at ~625 Hz; WFQ scheduler; priority inheritance
- PS/2 keyboard driver with scancode translation
- Framebuffer graphics with text console
- VFS with ramdisk, longest-prefix mount matching, page cache
- ELF loader for statically linked userspace binaries
- 19 syscalls: process (exit, yield, getpid, getppid, spawn, wait), fd I/O (open, close, read, write, seek, dup, dup2, pipe, getdents), IPC (send, recv, recv_block, create_port, create_notification_port, notify), filesystem (mkdir, unlink, rmdir, format), memory (mmap, munmap)
- FAT32 filesystem with mkdir/unlink, LFN support
- ext4 Ring-3 daemon (`ext4d`) with kernel IPC stubs (superblock, inode, extent tree, read/write/mkdir/unlink)
- VirtIO 1.0 block device driver with PCI enumeration
- IPC port-based message passing with blocking receive and async notification ports
- LES (Least-Elevated Security) bridge: declarative policy engine + audit log
- /proc/serix/cap-audit pseudo-file
- Kernel shell (`kshell`) with 11 built-in commands

## Building

Prerequisites:

```
  rustup default nightly
  rustup target add x86_64-unknown-none
  apt install make qemu-system-x86 xorriso git
```

Quick start:

```
  git clone https://github.com/gitcomit8/serix.git
  cd serix
  git submodule update --init --recursive
  make run
```

This builds the kernel, creates a bootable ISO with Limine, and launches
QEMU with serial output redirected to stdio and an emulated Intel IOMMU.
Successful boot displays a blue framebuffer with memory map visualization;
serial logs remain on stdio. The kernel keeps the newest 32 KiB in memory;
kshell `logs` displays the latest 4 KiB. Use `clear` to clear the framebuffer
console.

[Asciinema recording placeholder: Build process from clean state]
Alt text: "Terminal recording showing complete build process: cargo build output, make iso creating bootable image, and QEMU launch with kernel booting to blue screen, approximately 60 seconds"

Build targets:

```
  make iso          # Build bootable ISO (serix.iso)
  make run          # Build and run in QEMU
  make clean        # Remove build artifacts
  cargo fmt         # Format code (tabs, 100 char lines)
  cargo clippy      # Lint with Clippy
```

## Testing

No automated test suite exists. Validation is manual:

 1. Boot in QEMU (`make run`)
 2. Verify serial output shows all initialization checkpoints
 3. Verify blue framebuffer appears with memory map bars
 4. Test keyboard input (characters appear on framebuffer console)
 5. Test kshell commands: `help`, `ls`, `cat`, `mkdir`, `rm`, `logs`, `clear`
 6. Verify timer interrupts via tick count increments in serial output

Some crates have unit tests runnable via `cargo test` (e.g., `capability/`, `task/`).

[Asciinema recording placeholder: Keyboard input test]
Alt text: "Terminal recording showing keyboard input test - typing characters on QEMU window and seeing them appear on framebuffer console with scancode/ASCII output on serial console, approximately 20 seconds"

## Repository Layout

```
  kernel/         Kernel entry point, syscalls, GDT, process spawning, kshell, LES bridge
  memory/         Page tables, heap allocator, frame allocator, kstack, VMA manager
  hal/            Hardware abstraction (serial, CPU topology, I/O ports, MSRs)
  apic/           APIC interrupt controller (Local APIC, I/O APIC, timer)
  idt/            Interrupt descriptor table and exception handlers
  graphics/       Framebuffer console, drawing primitives
  task/           Async task executor, scheduler, context_switch, WFQ
  capability/     Capability-based security (store, cspace, audit ring, validation)
  ipc/            Port-based IPC, message passing, async notification ports
  drivers/        Device drivers (VirtIO block, PCI enumeration)
  vfs/            Virtual filesystem with INode abstraction, mount table, page cache
  fs/             Filesystem drivers (FAT32, ext2, ext4 parser/stubs, policy engine)
  keyboard/       PS/2 keyboard driver
  loader/         ELF userspace binary loader
  ulib/           Userspace library with syscall wrappers and capability refs
  util/           Panic handler, shared utilities
  ext4d/          Ring-3 ext4 filesystem daemon
  policyd/        Ring-3 policy service daemon
  docs/           Technical documentation
  limine/         Limine bootloader (git submodule)
```

**Module Documentation**: [kernel](kernel/README.md) | [memory](memory/README.md) | [hal](hal/README.md) | [apic](apic/README.md) | [idt](idt/README.md) | [graphics](graphics/README.md) | [task](task/README.md) | [util](util/README.md) | [keyboard](keyboard/README.md) | [vfs](vfs/README.md) | [ipc](ipc/README.md) | [drivers](drivers/README.md) | [fs](fs/README.md)

## Documentation

- **Technical Documentation** ([docs/](docs/))
  - [Architecture Overview](docs/ARCHITECTURE.md) - System architecture, subsystem organization, ring model
  - [Boot Process](docs/BOOT_PROCESS.md) - Boot sequence and initialization
  - [Memory Layout](docs/MEMORY_LAYOUT.md) - Virtual memory layout and addressing
  - [Interrupt Handling](docs/INTERRUPT_HANDLING.md) - IDT, APIC, and interrupt flow
  - [Kernel API](docs/KERNEL_API.md) - Syscall interface and usage
  - [Graphics API](docs/GRAPHICS_API.md) - Framebuffer operations
  - [HAL API](docs/HAL_API.md) - Hardware abstraction layer
  - [Roadmap](docs/ROADMAP.md) - Development roadmap and milestones
- **Developer Guide**
  - [Contributing Guidelines](CONTRIBUTING.md) - Code style, build, and PR process

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for:

- Code style guidelines (tabs, Linux kernel comment style)
- Build and test procedures
- Commit message format
- Pull request process
- Areas needing work (see [Phase 4 of roadmap](docs/ROADMAP.md))

Bug reports and feature requests should use GitHub issue templates in
[.github/ISSUE_TEMPLATE/](.github/ISSUE_TEMPLATE/).

## License

GNU General Public License v3.0. See [LICENSE](LICENSE) file.
