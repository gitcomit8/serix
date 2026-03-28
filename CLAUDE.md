# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Serix is a hybrid-kernel x86_64 operating system written in Rust (`#![no_std]`, nightly toolchain). It boots via the Limine v10.x bootloader and uses capability-based security. Currently at v0.0.6, Phase 4 development (FAT32 filesystem).

## Build Commands

```bash
cargo build --release          # Build kernel only
cargo build -p <crate> --release  # Build a specific crate
make init                      # Build userspace init binary (required before iso)
make iso                       # Build bootable ISO (kernel + init + Limine)
make run                       # Build everything and run in QEMU
make clean && cargo clean      # Remove all build artifacts
cargo fmt                      # Format (tabs, 100-char width)
cargo clippy                   # Lint
```

No automated test suite exists. Validate by booting in QEMU (`make run`) and checking serial output + framebuffer.

**QEMU debug flags**: `qemu-system-x86_64 ... -d int,cpu_reset -no-reboot` to catch triple faults.

## Architecture

### Workspace Structure

Cargo workspace with 16 member crates. `.cargo/config.toml` sets `x86_64-unknown-none` as default target and enables `build-std` — no `--target` flag needed.

Key crates: `kernel/` (entry, GDT, syscalls), `memory/` (paging, heap, frame allocator), `hal/` (serial, CPU, I/O ports), `apic/` (LAPIC, I/O APIC, timer), `idt/` (exception handlers), `graphics/` (framebuffer console), `task/` (async executor, scheduler), `capability/` (security), `drivers/` (VirtIO, PCI), `vfs/` (ramdisk), `ipc/` (port-based messaging), `loader/` (ELF loader), `ulib/` (userspace syscall wrappers), `keyboard/` (PS/2 driver).

Internal crate dependencies use `{ path = "../crate_name" }`.

### Memory Layout

- **HHDM**: All physical RAM mapped at `0xFFFF_8000_0000_0000` — always use `HHDM_REQ.get_response()` for offset, never hardcode
- **Kernel heap**: `0xFFFF_8000_4444_0000` (1 MB, configured in `memory/src/heap.rs`)
- **Userspace**: Lower half, entry at `0x200000` (via `user.ld` linker script)

### Boot Flow (kernel/src/main.rs `_start`)

Serial init → disable PIC/enable APIC → load IDT → enable interrupts/start LAPIC timer → process Limine responses → init page tables from CR3 → init heap → init graphics → init VFS → load init binary → idle loop.

**Critical ordering**: heap must exist before any allocations; IDT must be loaded before enabling interrupts.

### Interrupt Vectors

0-31: CPU exceptions, 32: PIT (disabled), 33: PS/2 keyboard, 49: LAPIC timer (~625 Hz). All handlers must signal EOI to APIC.

### Syscall Interface

Linux-style ABI via `SYSCALL`/`SYSRET`. RAX=number, RDI/RSI/RDX/R10/R8/R9=args. Dispatch in `kernel/src/syscall.rs`, wrappers in `ulib/src/lib.rs`. Syscalls: READ(0), WRITE(1), SEND(20), RECV(21), YIELD(24), EXIT(60).

### Task/Scheduler Model

`TaskCB` stores state + stack pointer. `Scheduler` with `RunQueue` (round-robin). `AsyncTask` with `Future` trait. Context switch via assembly in `task/` crate (callee-saved GPRs + CR3 swap).

## Code Conventions

- **Tabs, not spaces** (`hard_tabs = true`, `tab_spaces = 8`, `max_width = 100`)
- **C-style block comments** for function headers: `/* function_name - description */`
- **Global state pattern**: `static INSTANCE: Once<Mutex<Type>> = Once::new()`
- **Debug output**: `serial_println!` (kernel), `fb_println!` (framebuffer)
- **Address types**: Use `x86_64::PhysAddr` and `x86_64::VirtAddr`, not raw integers
- **Syscall naming**: prefix with `serix_` (e.g., `serix_write`)
- **Handler naming**: suffix with `_handler` (e.g., `timer_interrupt_handler`)
- **Commit format**: `<type>(<scope>): <subject>` — types: feat/fix/docs/style/refactor/perf/test/build/ci/chore, scopes: crate names

## Documentation

Each subsystem crate has its own `README.md`. Technical docs in `docs/` cover: `ARCHITECTURE.md`, `BOOT_PROCESS.md`, `MEMORY_LAYOUT.md`, `INTERRUPT_HANDLING.md`, `KERNEL_API.md`, `GRAPHICS_API.md`, `HAL_API.md`, `ROADMAP.md`.
