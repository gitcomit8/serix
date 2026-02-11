Serix Kernel
============

Serix is a microkernel-style x86_64 operating system written in Rust with
capability-based security. The kernel boots via the Limine bootloader and
provides a minimal userspace execution environment with VFS, IPC, and
preemptive scheduling.

Status
------
Current release: v0.0.5

The kernel successfully boots to a graphical console, initializes core
subsystems (APIC, IDT, heap, VFS), loads and executes a userspace init
binary, and responds to keyboard/timer interrupts.

[Screenshot placeholder: Boot screen with blue framebuffer and memory map visualization]
Alt text: "Serix kernel boot screen showing blue framebuffer with colored memory map bars at bottom and white text displaying boot messages"

[Asciinema recording placeholder: Complete boot sequence from QEMU start to init execution]
Alt text: "Terminal recording showing QEMU boot of Serix kernel with serial console output displaying all initialization checkpoints from serial init through init binary execution, approximately 30 seconds"

Features
--------
 * x86_64 long mode kernel with UEFI and BIOS support via Limine
 * Capability-based security with cryptographic capability handles
 * Physical memory management with boot-time frame allocator
 * Virtual memory with 4-level paging (PML4)
 * Heap allocator (linked_list_allocator, 1MB default)
 * APIC interrupt controller (Local APIC + I/O APIC, legacy PIC disabled)
 * Interrupt descriptor table with exception handlers
 * LAPIC timer at ~625 Hz
 * PS/2 keyboard driver with scancode translation
 * Framebuffer graphics with text console
 * VFS with ramdisk support
 * ELF loader for userspace binaries
 * Basic syscalls: write, read, exit, yield
 * Async task executor with cooperative scheduling

Building
--------
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
QEMU with serial output redirected to stdio. Successful boot displays a
blue framebuffer with memory map visualization and serial console output.

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

Testing
-------
No automated test suite exists yet. Validation is manual:

 1. Boot in QEMU (make run)
 2. Verify serial output shows all initialization checkpoints
 3. Verify blue framebuffer appears with memory map bars
 4. Test keyboard input (characters appear on framebuffer console)
 5. Observe timer interrupts (tick count increments in serial output)

[Asciinema recording placeholder: Keyboard input test]
Alt text: "Terminal recording showing keyboard input test - typing characters on QEMU window and seeing them appear on framebuffer console with scancode/ASCII output on serial console, approximately 20 seconds"

Repository Layout
-----------------
```

  kernel/         Kernel entry point, syscalls, GDT
  memory/         Page tables, heap allocator, frame allocator  
  hal/            Hardware abstraction (serial, CPU topology, I/O ports)
  apic/           APIC interrupt controller (Local APIC, I/O APIC, timer)
  idt/            Interrupt descriptor table and exception handlers
  graphics/       Framebuffer console, drawing primitives
  task/           Async task executor and scheduler
  capability/     Capability-based security system
  drivers/        Device drivers (VirtIO block, PCI, console)
  vfs/            Virtual filesystem with INode abstraction
  ipc/            Inter-process communication
  loader/         ELF userspace binary loader
  ulib/           Userspace library with syscall wrappers
  util/           Utility functions (panic handler, etc.)
  docs/           Technical documentation
  limine/         Limine bootloader (git submodule)
```

Documentation [docs/](docs/)
-----------------

```
  docs/BOOT_PROCESS.md         Boot sequence and initialization
  docs/MEMORY_LAYOUT.md        Virtual memory layout and addressing
  docs/INTERRUPT_HANDLING.md   IDT, APIC, and interrupt flow
  docs/KERNEL_API.md           Syscall interface and usage
  docs/GRAPHICS_API.md         Framebuffer operations
  docs/HAL_API.md              Hardware abstraction layer
  docs/ROADMAP.md              Development roadmap and milestones
  CONTRIBUTING.md              Contributor guidelines and style
```

Contributing
------------
Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for:

 * Code style guidelines (tabs, Linux kernel comment style)
 * Build and test procedures
 * Commit message format
 * Pull request process
 * Areas needing work (see docs/ROADMAP.md Phase 3)

Bug reports and feature requests should use GitHub issue templates in
.github/ISSUE_TEMPLATE/.

License
-------
GNU General Public License v3.0. See [LICENSE](LICENSE) file.

