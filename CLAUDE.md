# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with this repository.

## Project Overview

Serix is a hybrid-kernel x86_64 operating system written in `#![no_std]` Rust. It boots via the Limine v10.x bootloader, uses capability-based security, and currently supports SMP with up to 16 CPUs.

## Build Commands

```bash
cargo build --release          # Build all workspace crates (kernel + all subsystems)
cargo build -p <crate> --release  # Build a specific crate
make init                      # Build userspace init binary (required before iso)
make iso                       # Build bootable ISO (kernel + ext4d + Limine)
make run                       # Build everything and run in QEMU
make run-debug                 # Run with -d int,cpu_reset -no-reboot (catch triple faults)
make run-gdb                   # Run with GDB server on port 1234, paused (-S -s)
make clean                     # Remove ISO and kernel build artifacts
cargo fmt                      # Format (tabs, 100-char width)
cargo clippy                   # Lint
```

**No automated test suite.** Validate by booting in QEMU (`make run`) and checking serial output + framebuffer.

**QEMU debug flags:** `make run-debug` adds `-d int,cpu_reset -no-reboot` to catch triple faults.

## Architecture

### Workspace Structure

Cargo workspace with 16 member crates. `.cargo/config.toml` sets `x86_64-unknown-none` as default target and enables `build-std` — no `--target` flag needed.

Key crates: `kernel/` (entry, GDT, syscalls, process spawning), `memory/` (paging, heap, frame allocator, kernel stacks), `hal/` (serial, CPU, I/O ports), `apic/` (LAPIC, I/O APIC, timer), `idt/` (exception handlers), `graphics/` (framebuffer console), `task/` (async executor, scheduler, context switch), `capability/` (security), `drivers/` (VirtIO, PCI), `vfs/` (virtual filesystem), `ipc/` (port-based messaging), `loader/` (ELF loader), `ulib/` (userspace syscall wrappers), `fs/` (FAT32, ext2, ext4 parser/stubs), `keyboard/` (PS/2 driver), `ext4d/` (Ring 3 ext4 daemon), `util/` (panic handler).

Internal crate dependencies use `{ path = "../crate_name" }`.

### Memory Layout

- **HHDM**: All physical RAM mapped at `0xFFFF_8000_0000_0000` — always use `HHDM_REQ.get_response()` to get the offset, never hardcode
- **Kernel heap**: `0xFFFF_8000_4444_0000` (1 MB, configured in `memory/src/heap.rs`)
- **Userspace**: Lower half, entry at `0x200000` (via `user.ld` linker script)
- **Kernel stacks**: Allocated from `KSTACK_VA_START` (`0xFFFF_B000_0000_0000`) via `memory::kstack::alloc_kernel_stack()`

### Boot Flow (kernel/src/main.rs `_start`)

Serial init → disable PIC/enable APIC → register interrupt handlers → load IDT → enable interrupts → init executor → process Limine responses (framebuffer, memory map, HHDM) → init page tables from CR3 → init heap → init graphics → init VFS → spawn ext4d daemon → spawn kshell → init LAPIC timer → boot APs via Limine MP → idle loop.

**Critical ordering**: heap must exist before any allocations; IDT must be loaded before enabling interrupts; timer init must happen after scheduler init.

### Interrupt Vectors

0-31: CPU exceptions, 33: PS/2 keyboard, 49: LAPIC timer (~625 Hz). All handlers must signal EOI to APIC.

### Syscall Interface

Linux-style ABI via `SYSCALL`/`SYSRET`. RAX=number, RDI/RSI/RDX/R10/R8/R9=args. Dispatch in `kernel/src/syscall.rs`, wrappers in `ulib/src/lib.rs`.

| Number | Name | Description |
|--------|------|-------------|
| 0 | SYS_EXIT | Terminate calling task |
| 1 | SYS_YIELD | Voluntarily yield CPU |
| 2 | SYS_GETPID | Return calling task's ID |
| 3 | SYS_GETPPID | Return parent task's ID |
| 4 | SYS_SPAWN | Create new process from ELF path |
| 5 | SYS_WAIT | Wait for child to exit |
| 10 | SYS_OPEN | Open VFS path, return fd |
| 11 | SYS_CLOSE | Close fd |
| 12 | SYS_READ | Read from fd |
| 13 | SYS_WRITE | Write to fd |
| 14 | SYS_SEEK | Set file offset |
| 15 | SYS_DUP | Duplicate fd |
| 16 | SYS_DUP2 | Duplicate fd to specific number |
| 17 | SYS_PIPE | Create pipe, return [read_fd, write_fd] |
| 18 | SYS_GETDENTS | Read directory entries |
| 20 | SYS_MKDIR | Create directory |
| 21 | SYS_UNLINK | Delete file |
| 30 | SYS_SEND | Send IPC message to port |
| 31 | SYS_RECV | Receive IPC message (non-blocking) |
| 32 | SYS_RECV_BLOCK | Receive IPC message (blocking) |
| 33 | SYS_CREATE_PORT | Create IPC port, return capability handle |

**Error codes**: Negative errno values encoded as `u64::MAX - n` (e.g., `ERRNO_EBADF = u64::MAX - 8`).

**Userspace wrappers**: `ulib/src/lib.rs` provides `serix_read()`, `serix_write()`, etc.

### Task/Scheduler Model

`TaskCB` stores state + stack pointer + CPU context. `Scheduler` with per-CPU `RunQueue` (WFQ with virtual runtime). `AsyncTask` with `Future` trait. Context switch via assembly in `task/context_switch.rs` (callee-saved GPRs + CR3 swap + FS/GS_BASE MSRs).

**SMP**: Each CPU has its own `PerCpuData` (accessed via `GS_BASE` MSR). BSP initializes first, then Limine boots APs via `MpRequest` callback mechanism. APs spin-wait on `AP_READY_PTR` (Acquire ordering) until BSP signals init complete, then allocate kernel stacks, init scheduler, mask/unmask LAPIC timer, and enter idle loop.

**Priority Inheritance**: `TaskCB` has `inherited_priority` and `blocked_on` fields. `acquire_lock_with_pi()` boosts holder priority; `release_lock_with_pi()` restores it.

### ELF Loading & Process Spawning

- `kernel/src/process.rs::spawn_user_process()` parses ELF, allocates PML4, maps `PT_LOAD` segments, constructs user stack, enters Ring 3 via `iretq`
- Overlapping segments share pages (detected via `PageTableFlags::PRESENT` check)
- Userspace loaded at `0x200000` (set by `user.ld` linker script)
- ext4d daemon is embedded as ELF bytes in kernel binary (`include_bytes!`)

### Filesystem Stack

**Ring 0 (Kernel):**
- **VFS layer** (`vfs/`): Path resolution, FD table, `INode` trait
- **FAT32**: Full R/W, LFN support, cluster allocation
- **ext2**: Basic support
- **ext4 stub**: Kernel-side IPC forwarding to Ring 3 daemon

**Ring 3 (Userspace):**
- **ext4d daemon** (`ext4d/`): Handles ext4 operations via IPC (superblock, inode tables, extent trees, file I/O)

**IPC Protocol**: Synchronous request/response via kernel IPC ports. `SYS_SEND` from kernel → ext4d, `SYS_RECV` from ext4d. Message format: opcode + args (`fs/src/ext4/ipc_protocol.rs`).

### Kernel Shell (kshell)

Built-in TTY shell running as a Ring-0 task. Input: PS/2 keyboard. Output: framebuffer console. Commands: `help`, `echo`, `ls`, `cat`, `write`, `mkdir`, `rm`, `mount`, `umount`, `halt`, `reboot`. Supports output redirection (`>` and `>>`).

## Key Conventions

- **Tabs, not spaces** (`hard_tabs = true`, `tab_spaces = 8`, `max_width = 100`)
- **C-style block comments** for function headers: `/* function_name - description */`
- **Global state pattern**: `static INSTANCE: Once<Mutex<Type>> = Once::new()`
- **Debug output**: `serial_println!` (kernel), `fb_println!` (framebuffer), `kprintln!` (kshell)
- **Address types**: Use `x86_64::PhysAddr` and `x86_64::VirtAddr`, not raw integers
- **Syscall naming**: prefix with `serix_` (e.g., `serix_write`)
- **Handler naming**: suffix with `_handler` (e.g., `timer_interrupt_handler`)
- **Commit format**: `<type>(<scope>): <subject>` — types: feat/fix/docs/style/refactor/perf/test/build/ci/chore, scopes: crate names

## Common Pitfalls

- **Heap before allocations**: Never use `Vec`, `Box`, `String` before `init_heap()` is called
- **Interrupts after IDT**: Never enable interrupts (STI) before IDT is loaded
- **Serial initialization**: Always initialize serial console first for debug output
- **APIC EOI**: All interrupt handlers must signal EOI to APIC (`apic::send_eoi()`)
- **Per-CPU run queues**: Each CPU must call `scheduler::init()` with its own CPU ID; the `PER_CPU_RUN_QUEUES` array is initialized lazily per-slot
- **AP timer interrupts**: APs must mask the LAPIC timer (`apic::timer::mask_timer()`) before init and unmask after scheduler init to prevent premature timer IRQs
- **Memory ordering**: APs waiting on `AP_READY_PTR` must use `Acquire` load; BSP must use `Release` store
- **Frame allocator**: `StaticBootFrameAllocator` has no deallocation; frames cannot be freed
- **Kernel stack allocation**: Use `memory::kstack::alloc_kernel_stack()` for per-task stacks; the KSTACK region is pre-mapped and visible in all address spaces

## Debugging

- **Serial output**: Primary debugging mechanism. QEMU redirects to stdio with `-serial stdio`
- **Checkpoint pattern**: Use `serial_println!("[CHECKPOINT] description")` throughout initialization
- **Triple fault**: Usually means stack overflow, invalid page table access, or exception before IDT loaded
- **QEMU debug flags**: Use `-d int,cpu_reset -no-reboot` to catch triple faults
- **GDB**: `make run-gdb` starts QEMU paused; connect with `gdb target/x86_64-unknown-none/release/kernel` then `target remote :1234`

## Limine Bootloader

Serix uses **Limine v10.x** (binary branch). Key differences from GRUB:

- **Configuration**: Uses `limine.conf` (not `grub.cfg`)
- **Request/Response model**: Kernel declares requests in `.limine_reqs` section, bootloader populates responses
- **Requests used**: `BaseRevision`, `FramebufferRequest`, `MemoryMapRequest`, `HhdmRequest`, `MpRequest`
- **Getting responses**: `FRAMEBUFFER_REQ.get_response().expect("No framebuffer")`
- **SMP**: `MpRequest` provides `cpu_ct` (total CPUs) and `cpus()` (array of `Cpu` structs with `goto_address` callbacks for AP boot)

Limine documentation: https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md

## Workspace Structure

This is a **Cargo workspace** with 16 member crates. Key implications:

- **Default target**: `.cargo/config.toml` sets `x86_64-unknown-none` as default target and enables `build-std` — no `--target` flag needed
- **Shared dependencies**: Managed in root `Cargo.toml` `[workspace]` section
- **Dependency paths**: Internal crates use `{ path = "../crate_name" }` syntax
- **Unified `Cargo.lock`**: All crates share the same lock file at workspace root
- **Linker script**: Kernel uses `kernel/linker.ld` (configured via rustflags in `.cargo/config.toml`)

## Building Userspace Binaries

Userspace binaries are built separately with the `user.ld` linker script:

```bash
RUSTFLAGS="-C relocation-model=static -C link-arg=-Tuser.ld -C link-arg=-no-pie" \
  cargo build -p <crate> --release --target x86_64-unknown-none
```

The `ext4d` daemon is embedded into the kernel binary via `include_bytes!()` and loaded at boot.

## QEMU Configuration

The `make run` command launches QEMU with:
- **4GB RAM**: `-m 4G`
- **4 CPUs**: `-smp 4`
- **Serial**: `-serial stdio`
- **VirtIO block device**: `-drive file=disk.img,if=none,format=raw,id=x0 -device virtio-blk-pci,drive=x0,disable-legacy=on,disable-modern=off`

To modify QEMU settings, edit the `QEMU_COMMON` variable in `Makefile`.

## Development Workflow

```bash
make clean       # Remove stale artifacts
make run         # Rebuild kernel + ISO, boot in QEMU
# Serial output appears in terminal
# Press Ctrl+C to exit QEMU
```

Kernel initialization is hardcoded in `kernel/src/main.rs::_start()`. To change process startup order, edit that file and rebuild with `make run`.

## Project Status

**Current Phase:** 4 (Storage & Filesystem Stack)  
**Version:** 0.0.6  
**Status:** ext4 daemon MVP integrated; FAT32 complete; SMP with 4+ CPUs working

### What Works
- Boot kernel to blue framebuffer with memory map
- Spawn multiple Ring 3 processes (ext4d daemon, kshell)
- Mount FAT32 or ext4 filesystem from block device
- Read/write files via syscall-mediated filesystem operations
- PS/2 keyboard input, LAPIC timer interrupts (~625 Hz)
- SMP with Limine MP: boot 1-16 CPUs, per-CPU run queues, per-CPU kernel stacks
- Priority inheritance protocol for lock contention
- Preemptive scheduling via LAPIC timer

### What's Missing
- Mount table (BTreeMap for multi-filesystem layouts)
- Auto-mount root filesystem at boot
- Ring 3 driver server framework (MMIO BAR mapping)
- ext4 journal (JBD2) and HTree directory indexing
- Unified page cache with demand paging
- Dynamic linking or `execve()`
- fork/clone/waitpid()
- Signal handling
