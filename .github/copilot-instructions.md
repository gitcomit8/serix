# Copilot Instructions for Serix Kernel

## Build, Test, and Run

### Building and Running

```bash
# Build kernel only
cargo build --release --manifest-path kernel/Cargo.toml --target x86_64-unknown-none

# Build bootable ISO (includes kernel + init binary + Limine bootloader)
make iso

# Build and run in QEMU (includes serial output via stdio)
make run

# Clean build artifacts
make clean
cargo clean
```

### Code Quality

```bash
# Format code (uses tabs, not spaces - Linux kernel style)
cargo fmt

# Run Clippy linter
cargo clippy --target x86_64-unknown-none
```

**Note**: No test suite exists yet. The kernel is validated by booting in QEMU and verifying:
- Serial console output shows initialization messages
- Blue framebuffer appears with memory map visualization
- Keyboard and timer interrupts work

## Architecture

### High-Level Design

Serix is a **microkernel-style x86_64 OS** written in Rust with these key architectural decisions:

- **Capability-based security**: All resource access is mediated through cryptographic capabilities stored in `CapabilityStore`
- **Workspace-based cargo project**: Kernel and subsystems are separate crates (`kernel/`, `memory/`, `hal/`, `apic/`, `idt/`, `graphics/`, `task/`, `capability/`, `drivers/`, `vfs/`, `ipc/`, `loader/`, `ulib/`)
- **Limine bootloader**: Uses Limine v10.x boot protocol (not GRUB). Limine sets up initial paging, framebuffer, and memory map before jumping to kernel
- **Physical memory mapping**: All physical RAM is mapped at virtual offset `0xFFFF_8000_0000_0000` (HHDM - Higher Half Direct Map)
- **Heap location**: Kernel heap lives at `0xFFFF_8000_4444_0000` (1 MB by default, configured in `memory/src/heap.rs`)

### Boot Flow

1. **Firmware (BIOS/UEFI)** → 2. **Limine bootloader** → 3. **Kernel `_start()` at `kernel/src/main.rs`**

The kernel entry point (`_start`) executes this initialization sequence:
1. Initialize serial console (COM1 0x3F8) for debug output
2. Disable legacy PIC, enable APIC (Local APIC + I/O APIC)
3. Load IDT with exception/interrupt handlers
4. Enable interrupts and start LAPIC timer
5. Process Limine responses (framebuffer, memory map, HHDM)
6. Initialize page tables using bootloader's CR3
7. Initialize heap using static boot frame allocator
8. Initialize graphics console and paint screen blue
9. Initialize VFS with ramdisk
10. Load and execute init binary from ramdisk
11. Enter idle loop (`hlt` instruction)

**Critical**: Heap must be initialized before any allocations (`Vec`, `Box`, etc.). Interrupts must be enabled after IDT is loaded.

### Memory Layout

- **Physical memory offset (HHDM)**: `0xFFFF_8000_0000_0000`
- **Kernel heap**: `0xFFFF_8000_4444_0000` - `0xFFFF_8000_4454_0000` (1 MB)
- **Kernel code**: High virtual addresses (loaded by Limine at `-2GB` from top typically)

To convert physical to virtual: `virt = phys + HHDM_offset`

See `docs/MEMORY_LAYOUT.md` for complete memory map.

### Subsystem Overview

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `kernel/` | Entry point, initialization, syscalls | `main.rs`, `syscall.rs`, `gdt.rs` |
| `hal/` | Hardware abstraction (serial, CPU topology) | `serial.rs`, `cpu.rs`, `topology.rs` |
| `apic/` | APIC interrupt controller (Local APIC, I/O APIC, timer) | `lib.rs`, `ioapic.rs`, `timer.rs` |
| `idt/` | Interrupt Descriptor Table setup | `lib.rs` |
| `memory/` | Page tables, heap, frame allocation | `lib.rs`, `heap.rs` |
| `graphics/` | Framebuffer console, drawing primitives | `lib.rs`, `console/mod.rs` |
| `task/` | Async task executor, scheduler skeleton | `lib.rs` |
| `capability/` | Capability-based security system | `lib.rs`, `store.rs`, `types.rs` |
| `drivers/` | Device drivers (VirtIO block, PCI, console) | `virtio.rs`, `pci.rs`, `console.rs` |
| `vfs/` | Virtual filesystem (ramdisk, INode abstraction) | `lib.rs` |
| `ipc/` | Inter-process communication | `lib.rs` |
| `loader/` | ELF loader for userspace binaries | `lib.rs` |
| `ulib/` | Userspace library (syscall wrappers) | Examples in `examples/` |

### Interrupt Handling

- **Vector allocation**:
  - 0-31: CPU exceptions (divide-by-zero, page fault, etc.)
  - 32: PIT timer (legacy, disabled)
  - 33: Keyboard (PS/2)
  - 49: LAPIC timer (periodic, ~625 Hz)
- **Handlers**: Defined in `idt/src/lib.rs`
- **APIC required**: Legacy PIC is disabled in `apic::enable()`

### Task Model

Currently skeletal. Tasks are async-based:
- `TaskCB` (Task Control Block) stores task state
- `Scheduler` is a placeholder (not preemptive yet)
- `init_executor()` sets up async executor
- Userspace tasks loaded via ELF loader (`loader/`)

## Key Conventions

### Code Style

- **Tabs, not spaces**: Configured in `rustfmt.toml` (`hard_tabs = true`, `tab_spaces = 8`)
- **100-character line width**: `max_width = 100` in `rustfmt.toml`
- **C-style comments for functions**: Use block comments `/* */` for function headers (see `kernel/src/main.rs`)
- **`serial_println!` for debug output**: Prefer this over `println!` (which doesn't exist in `no_std`)
- **`fb_println!` for framebuffer output**: After graphics initialization

### Rust Patterns

- **`#![no_std]` everywhere**: No standard library (kernel environment)
- **`extern crate alloc`**: Use after heap initialization for `Vec`, `Box`, `String`
- **`unsafe` blocks**: Common for hardware access (I/O ports, MSRs, raw pointers to MMIO)
- **`static` + `Once`/`Mutex`**: Pattern for global state (see `CAP_STORE_ONCE` in `kernel/src/main.rs`)
- **`lazy_static!` alternative**: Use `spin::Once` for one-time initialization

### Memory Safety

- **Physical addresses**: Use `x86_64::PhysAddr` type
- **Virtual addresses**: Use `x86_64::VirtAddr` type
- **Frame allocation**: Use `FrameAllocator` trait (e.g., `StaticBootFrameAllocator`)
- **Page table access**: Use `x86_64::structures::paging::Mapper` trait
- **HHDM offset**: Always use `HHDM_REQ.get_response()` to get physical memory offset, never hardcode

### Naming Conventions

- **Subsystem crates**: Lowercase, single word (`apic`, `idt`, `vfs`)
- **Syscall prefix**: All syscalls start with `serix_` (e.g., `serix_write`, `serix_exit`)
- **Interrupt handlers**: Suffix with `_handler` (e.g., `timer_interrupt_handler`)

### Common Pitfalls

- **Heap before allocations**: Never use `Vec`, `Box`, `String` before `init_heap()` is called
- **Interrupts after IDT**: Never enable interrupts (STI) before IDT is loaded
- **Serial initialization**: Always initialize serial console first for debug output
- **Framebuffer access**: Check Limine response is `Some` before accessing framebuffer
- **APIC EOI**: All interrupt handlers must signal EOI to APIC (see `apic/src/timer.rs`)

### Debugging

- **Serial output**: Primary debugging mechanism. QEMU redirects to stdio with `-serial stdio`
- **Checkpoint pattern**: Use `serial_println!("[CHECKPOINT] description")` throughout initialization
- **Triple fault**: Usually means stack overflow, invalid page table access, or exception before IDT loaded
- **QEMU debug flags**: Use `-d int,cpu_reset -no-reboot` to catch triple faults

## Limine Bootloader

Serix uses **Limine v10.x** (binary branch). Key differences from GRUB:

- **Configuration**: Uses `limine.conf` (not `grub.cfg`)
- **Request/Response model**: Kernel declares requests in `.limine_reqs` section, bootloader populates responses
- **Requests used**: `BaseRevision`, `FramebufferRequest`, `MemoryMapRequest`, `HhdmRequest`
- **Getting responses**: `FRAMEBUFFER_REQ.get_response().expect("No framebuffer")`

Limine documentation: https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md

## Workspace Structure

This is a **Cargo workspace** with 15+ member crates. Key implications:

- **Shared dependencies**: Managed in root `Cargo.toml` `[workspace]` section
- **Build commands**: Use `--manifest-path` to build specific crates (e.g., `cargo build --manifest-path kernel/Cargo.toml`)
- **Dependency paths**: Internal crates use `{ path = "../crate_name" }` syntax
- **Unified `Cargo.lock`**: All crates share the same lock file at workspace root

## Building Init Binary

The `init` binary (userspace) is built separately:

```bash
make init
# Internally runs:
# RUSTFLAGS="-C link-arg=-Tuser.ld" cargo build -p ulib --example init --release --target x86_64-unknown-none
```

This is required before `make iso` as the ISO includes the init binary in the ramdisk.

## QEMU Configuration

The `make run` command launches QEMU with specific devices:

- **4GB RAM**: `-m 4G`
- **Serial**: `-serial stdio` (redirected to terminal)
- **VirtIO block device**: `-drive file=disk.img,if=none,format=raw,id=x0 -device virtio-blk-pci,drive=x0`

To modify QEMU settings, edit the `run` target in `Makefile`.
