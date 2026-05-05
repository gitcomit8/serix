# Copilot Instructions for Serix Kernel

## Build, Test, and Run

### Building and Running

`.cargo/config.toml` sets `x86_64-unknown-none` as the default target and enables `build-std` for `core`, `alloc`, and `compiler_builtins`, so `--target` flags are not needed.

```bash
# Build kernel only
cargo build --release

# Build a specific crate
cargo build -p apic --release

# Build init binary (userspace) — required before `make iso`
make init

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
# Format code (uses tabs, not spaces — configured in rustfmt.toml)
cargo fmt

# Run Clippy linter
cargo clippy
```

### Commit Messages

Follow conventional commits: `<type>(<scope>): <subject>`

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`
Scopes: crate names — `kernel`, `memory`, `apic`, `idt`, `graphics`, `hal`, `task`, etc.

Example: `feat(apic): add LAPIC timer interrupt handler`

### Testing

No automated test suite exists. The kernel is validated by booting in QEMU (`make run`) and verifying:
- Serial console output shows initialization checkpoints
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
10. Spawn ext4d daemon (Ring 3 process handling ext4 IPC requests)
11. Spawn kshell interactive shell
12. Enter idle loop (`hlt` instruction)

**Critical ordering**: heap must exist before any allocations (`Vec`, `Box`, etc.). Interrupts must be enabled after IDT is loaded. ext4d must spawn before kshell so filesystem operations work.

### Memory Layout

- **Physical memory offset (HHDM)**: `0xFFFF_8000_0000_0000`
- **Kernel heap**: `0xFFFF_8000_4444_0000` - `0xFFFF_8000_4454_0000` (1 MB)
- **Kernel code**: High virtual addresses (loaded by Limine at `-2GB` from top typically)

To convert physical to virtual: `virt = phys + HHDM_offset`

See `docs/MEMORY_LAYOUT.md` for complete memory map.

### Subsystem Overview

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `kernel/` | Entry point, initialization, syscalls, boot orchestration | `main.rs`, `syscall.rs`, `gdt.rs`, `process.rs` |
| `hal/` | Hardware abstraction (serial, CPU topology, I/O ports) | `serial.rs`, `cpu.rs`, `topology.rs` |
| `apic/` | APIC interrupt controller (Local APIC, I/O APIC, timer) | `lib.rs`, `ioapic.rs`, `timer.rs` |
| `idt/` | Interrupt Descriptor Table setup | `lib.rs` |
| `memory/` | Page tables, heap, frame allocation, HHDM management | `lib.rs`, `heap.rs`, `page_table.rs` |
| `graphics/` | Framebuffer console, drawing primitives, memory map visualization | `lib.rs`, `console/mod.rs` |
| `task/` | Async task executor, scheduler skeleton, TaskCB state | `lib.rs` |
| `capability/` | Capability-based security system (framework, not yet enforced) | `lib.rs`, `store.rs`, `types.rs` |
| `keyboard/` | PS/2 keyboard driver, scancode translation | `lib.rs` |
| `drivers/` | Device drivers (VirtIO block, PCI enumeration, console) | `virtio.rs`, `pci.rs`, `console.rs` |
| `fs/` | Filesystem drivers (FAT32, ext2, ext4 daemon stub) with mount registry | `lib.rs`, `fat32/`, `ext2/`, `ext4/`, `block_cache.rs` |
| `vfs/` | Virtual filesystem (path resolution, FD table, INode trait) | `lib.rs` |
| `ipc/` | Inter-process communication (port-based synchronous messaging) | `lib.rs` |
| `loader/` | ELF loader for userspace binaries (segment mapping, relocation) | `lib.rs` |
| `ext4d/` | **Ring 3 ext4 filesystem daemon** (handles IPC requests from kernel) | `main.rs` |
| `ulib/` | Userspace library (syscall wrappers, init binary example) | `lib.rs`, `examples/init.rs` |

### Interrupt Handling

- **Vector allocation**:
  - 0-31: CPU exceptions (divide-by-zero, page fault, etc.)
  - 32: PIT timer (legacy, disabled)
  - 33: Keyboard (PS/2)
  - 49: LAPIC timer (periodic, ~625 Hz)
- **Handlers**: Defined in `idt/src/lib.rs`
- **APIC required**: Legacy PIC is disabled in `apic::enable()`

### Syscall Interface

System calls use `SYSCALL`/`SYSRET` instructions with Linux-style register ABI (`rax`=nr, `rdi`=arg1, `rsi`=arg2, `rdx`=arg3, `r10`=arg4, `r8`=arg5).

| Number | Name | Description |
|--------|------|-------------|
| 0 | `SYS_READ` | Read from fd (only fd 0 / STDIN) |
| 1 | `SYS_WRITE` | Write to fd (only fd 1 / STDOUT) |
| 5 | `SYS_OPEN` | Open file (returns fd) |
| 3 | `SYS_CLOSE` | Close fd |
| 8 | `SYS_SEEK` | Seek within file |
| 20 | `SYS_SEND` | Send IPC message to port (ext4d uses) |
| 21 | `SYS_RECV` | Receive IPC message from port (ext4d uses) |
| 24 | `SYS_YIELD` | Yield CPU voluntarily |
| 60 | `SYS_EXIT` | Terminate process |
| 83 | `SYS_MKDIR` | Create directory |
| 87 | `SYS_UNLINK` | Delete file or empty directory |

Kernel-side dispatch: `kernel/src/syscall.rs`. Userspace wrappers: `ulib/src/lib.rs`.

### Task Model

Currently skeletal. Tasks are async-based:
- `TaskCB` (Task Control Block) stores task state, stack, CR3, and registers
- `Scheduler` is a placeholder (not preemptive yet)
- `init_executor()` sets up async executor
- Userspace tasks loaded via ELF loader (`loader/`)

### ELF Loader & Process Spawning

- **Segment mapping**: `kernel/src/process.rs` maps PT_LOAD segments into user address space
- **Overlapping segments**: When multiple segments share a page (e.g., `.got` and `.rodata` on same 4KB page), the loader detects existing mappings via `translate_addr()` and reuses the mapped frame, updating page flags as needed
- **User entry trampoline**: Naked assembly function at `user_entry_trampoline()` bridges Ring 0 execution to Ring 3 via `iretq`
- **Userspace address space**: Loaded at 0x200000 (set by `user.ld` linker script)

### Filesystem Stack

**Ring 0 (Kernel):**
- **VFS layer** (`vfs/`): Path resolution, file descriptor table, INode trait abstraction
- **Filesystem drivers** (`fs/`):
  - **FAT32**: Full R/W, LFN support, cluster allocation (all in kernel)
  - **ext2**: Basic ext2 support (legacy)
  - **ext4 daemon stub** (`fs/ext4/kernel_stub.rs`): Kernel-side IPC forwarding layer (translates INode operations to IPC messages)
- **Block device abstraction**: `BlockDev` trait for sector-level I/O; `CachedBlockDev` for write-through caching

**Ring 3 (Userspace):**
- **ext4d daemon** (`ext4d/`): Runs as isolated Ring 3 process
  - Handles ext4 operations via IPC receive/send
  - Parses superblock, BGDT, inode tables, extent trees
  - Handles file read/write, directory lookup, metadata ops
  - **Current scope**: Linear directories, extent-based files, basic metadata (no journal, no HTree indexing)

**IPC Protocol:**
- Synchronous request/response via kernel IPC ports
- `SYS_SEND(port_id, message)` from kshell → ext4d
- `SYS_RECV(port_id, &mut message)` from ext4d to receive kernel requests
- Message format: opcode + args (defined in `fs/src/ext4/ipc_protocol.rs`)

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

This is a **Cargo workspace** with 16 member crates. Key implications:

- **Default target**: `.cargo/config.toml` sets `x86_64-unknown-none` as default target and enables `build-std` — no `--target` flag needed
- **Shared dependencies**: Managed in root `Cargo.toml` `[workspace]` section
- **Dependency paths**: Internal crates use `{ path = "../crate_name" }` syntax
- **Unified `Cargo.lock`**: All crates share the same lock file at workspace root
- **Linker script**: Kernel uses `kernel/linker.ld` (configured via rustflags in `.cargo/config.toml`)

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
- **Additional image**: `ext4.img` for ext4 testing (formatted with `mkfs.ext4 -O ^has_journal,^64bit,^metadata_csum,^dir_index`)

To modify QEMU settings, edit the `run` target in `Makefile`.

## Writing Serix Userspace Binaries

### Current Capabilities (Phase 4)

Userspace binaries are loaded as Ring 3 ELF executables. Only static linking is supported (no dynamic linker yet).

**Requirements:**
- Write in Rust (requires `ulib` syscall wrappers)
- Compile with `x86_64-unknown-none` target (baremetal)
- Link with `user.ld` linker script
- Use `#![no_std]` and `extern crate alloc`

### Building an Example Binary

The init binary is built separately:
```bash
make init
# Internally:
# RUSTFLAGS="-C relocation-model=static -C link-arg=-Tuser.ld -C link-arg=-no-pie" \
#   cargo build -p ulib --example init --release --target x86_64-unknown-none
```

### Linking Against ulib

```rust
// myapp/src/main.rs
#![no_std]
#![no_main]

extern crate alloc;
use ulib::*;

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    serix_write(1, b"Hello from userspace!\n");
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
```

Compile with:
```bash
RUSTFLAGS="-C relocation-model=static -C link-arg=-Tserix/user.ld -C link-arg=-no-pie" \
  cargo build --target x86_64-unknown-none --release
```

**Note**: Future phases will add support for C via musl, but currently only Rust + ulib is stable.

## Development Workflow & Testing

### Quick Testing Loop
```bash
make clean       # Remove stale artifacts
make run         # Rebuild kernel + ISO, boot in QEMU
# Serial output appears in terminal
# Press Ctrl+C to exit QEMU
```

### Debugging a Crash
```bash
make run-debug   # Adds -d int,cpu_reset -no-reboot to catch triple faults
# Look for "Reset" or "Triple fault" in output
# Use serial checkpoints ([CHECKPOINT] messages) to narrow down failing subsystem
```

### Modifying Boot Sequence
Kernel initialization is hardcoded in `kernel/src/main.rs::_start()`. Current order:
1. ext4d daemon spawned (Ring 3 process)
2. kshell spawned (interactive shell)

To change process startup order, edit `kernel/src/main.rs` and rebuild with `make run`.

## Project Status & Roadmap

**Current Phase:** 4 (Storage & Filesystem Stack)  
**Version:** 0.0.6  
**Status:** ext4 daemon MVP integrated; FAT32 complete

### What Works
- Boot kernel to blue framebuffer with memory map
- Spawn multiple Ring 3 processes (ext4d daemon, kshell)
- Mount FAT32 or ext4 filesystem from block device
- Read/write files via syscall-mediated filesystem operations
- PS/2 keyboard input, LAPIC timer interrupts
- Basic shell commands (help, echo, etc.)

### What's Missing (Phase 4 continuation)
- Mount table (BTreeMap for multi-filesystem layouts)
- PCI device enumeration (auto-detect block devices → /dev names)
- Auto-mount root filesystem at boot
- Ring 3 driver server framework (MMIO BAR mapping)
- ext4 journal (JBD2) and HTree directory indexing
- Unified page cache with demand paging

### Known Limitations
- ext4d daemon is MVP scope: linear directories only (no HTree), single-level extents, no journal
- FAT32 bypasses block cache (uses global `read_sector()`/`write_sector()`)
- No dynamic linking or `execve()` yet
- No fork/clone/waitpid() yet
- No signal handling
