# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with this repository.

## Project Overview

Serix is a hybrid-kernel x86_64 operating system written in `#![no_std]` Rust. It boots via the Limine v10.x bootloader, uses capability-based security, and currently supports SMP with up to 16 CPUs.

## Build Commands

```bash
cargo build --release          # Build all workspace crates (kernel + all subsystems)
cargo build -p <crate> --release  # Build a specific crate
make init                      # Build userspace init binary (required before iso)
make iso                       # Build bootable ISO (kernel + ext4d + policyd + Limine)
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

Cargo workspace with 20 member crates. `.cargo/config.toml` sets `x86_64-unknown-none` as default target and enables `build-std` — no `--target` flag needed.

Key crates: `kernel/` (entry, GDT, syscalls, process spawning, LES bridge), `memory/` (paging, heap, frame allocator, kernel stacks, VMAs), `hal/` (serial, CPU, I/O ports, topology), `acpi/` (RSDP/RSDT/XSDT and MADT parsing), `apic/` (LAPIC, I/O APIC, x2APIC, IPIs, timer), `idt/` (exception handlers), `graphics/` (framebuffer console), `task/` (async executor, per-CPU WFQ scheduler, context switch), `capability/` (C-space, rights, validation, audit), `drivers/` (VirtIO, PCI), `vfs/` (mount table, path resolution, page cache), `ipc/` + `ipc_types/` (port-based messaging and wire format), `loader/` (ELF loader), `ulib/` (userspace syscall wrappers), `fs/` (FAT32, ext2, ext4 with JBD2 journal and HTree indexing, policy rules), `keyboard/` (PS/2 driver), `ext4d/` (Ring 3 ext4 daemon), `policyd/` (Ring 3 policy daemon), `util/` (panic handler).

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

Linux-style `SYSCALL`/`SYSRET` entry with Serix-native numbers (not Linux-compatible). RAX=number, RDI/RSI/RDX/R10/R8/R9=args. Dispatch in `kernel/src/syscall.rs`, wrappers in `ulib/src/lib.rs`. File, memory, and IPC operations pass a capability reference (`slot` + `generation`) in their leading arguments; process controls (`SPAWN`, `WAIT`) authorize against the calling task's capability space. `authorization_for(nr)` is the exhaustive per-arm table classifying each syscall as `Unprotected` or requiring specific `Rights`.

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
| 22 | SYS_RMDIR | Remove directory |
| 23 | SYS_FORMAT | Format block device as ext4 (needs PLATFORM_ADMIN) |
| 30 | SYS_SEND | Send IPC message to port |
| 31 | SYS_RECV | Receive IPC message (non-blocking) |
| 32 | SYS_RECV_BLOCK | Receive IPC message (blocking, fills full `Message`) |
| 33 | SYS_CREATE_PORT | Create IPC port, return capability handle |
| 34 | SYS_CREATE_NOTIFICATION_PORT | Create async notification port with event bitmask |
| 35 | SYS_NOTIFY | Raise event bitmask on a notification port |
| 90 | SYS_MMAP | Map a file into the task's address space |
| 91 | SYS_MUNMAP | Unmap a VMA |
| 92 | SYS_FILE_READ | Read a whole file (capped at 4 KiB) into a user buffer |
| 93 | SYS_POLICY_RELOAD | Reparse policy config and swap the LES snapshot |

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
- `ext4d` and `policyd` are embedded as ELF bytes in the kernel binary (`include_bytes!`) and inserted into the VFS root at boot

### Filesystem Stack

**Ring 0 (Kernel):**
- **VFS layer** (`vfs/`): Mount table (`MOUNT_TABLE`, a `BTreeMap` iterated in reverse for longest-prefix matching), `mount()`/`umount()`/`lookup_path()`, FD table, `INode` trait, `RamDir`/`RamFile`, page cache, mmap-backed VMAs
- **Boot mounts**: `RamDir` at `/` and `/dev/`, `/dev/sda` for VirtIO-blk, `/proc/serix/cap-audit`, `/etc/serix/policy.toml`; `init::scheduler::init_scheduler()` probes `/dev/sda` and auto-mounts the detected filesystem at `/mnt` (RamDir fallback if detection fails)
- **FAT32**: Full R/W, LFN support, cluster allocation
- **ext2**: Basic support
- **ext4**: Kernel-side stub forwards operations to the Ring 3 daemon over IPC; also implements superblock/BGDT/inode/extent/directory parsing, JBD2 journal writeback and crash recovery (`fs/src/ext4/journal.rs`), HTree directory indexing (`fs/src/ext4/htree.rs`), and formatting via `SYS_FORMAT` (`fs/src/ext4/format.rs`)

**Ring 3 (Userspace):**
- **ext4d daemon** (`ext4d/`): Serves ext4 operations over IPC (superblock, inode tables, extent trees, directories, file I/O)
- **policyd daemon** (`policyd/`): Stores and evaluates path-based policy loaded from `/etc/serix/policy.toml`

**IPC Protocol**: Request/response over queued 128-byte port messages — `SYS_SEND` enqueues and never blocks on the receiver. Kernel → ext4d on `EXT4_REQ_PORT` (`0x4E00`), replies on `EXT4_REPLY_BASE + sender_id` (`0x4E01`); policyd listens on `0x5000`. Message format: opcode + args (`fs/src/ext4/ipc.rs`).

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

## Search Conventions

**Always use Serena for codebase searches** — never fall back to `grep`, `rg` (ripgrep), `find`, or shell-based text search tools. Serena provides semantic symbol search, reference finding, declaration navigation, and diagnostics via its LSP-backed index.

- **Find a symbol / function / struct**: Use Serena's `find_symbol` or `get_symbols_overview`
- **Find usages / references**: Use Serena's `find_referencing_symbols`
- **Navigate to declaration**: Use Serena's `find_declaration`
- **Find implementations**: Use Serena's `find_implementations`
- **File search**: Use Serena's `find_file` or `list_dir`
- **Pattern search**: Use Serena's `search_for_pattern`
- **Diagnostics**: Use Serena's `get_diagnostics_for_file` or `get_diagnostics_for_symbol`

Only revert to shell tools if Serena is unavailable or explicitly requested otherwise.

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

This is a **Cargo workspace** with 20 member crates. Key implications:

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

The `ext4d` and `policyd` daemons are embedded into the kernel binary via `include_bytes!()` and loaded at boot.

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

## x86-64 Hardware Reference

When referencing the Intel SDM for architectural details — register layouts, instruction encodings, memory model rules, interrupt descriptors — **always invoke the `x86-sdm` skill first**. It provides structured guidance for low-level x86 work.

- **Skill**: `.claude/skills/x86-sdm/SKILL.md` — invoke with `/x86-sdm` when implementing/debugging low-level x86 code
- **Converted specs**: `docs/specs/intel-sdm/INDEX.md` — chapter-split Markdown from Intel SDM Vol. 3
- **Verified notes**: `docs/hw-notes/` — your own distilled register layouts with SDM citations
- **Page verifier**: `python3 scripts/sdm_page.py <vol> <pdf_page>` renders a PDF page to PNG
- Always check `docs/hw-notes/` first, then the converted spec, then verify bit positions via `sdm_page.py`

## Project Status

**Current Phase:** 4 (Storage & Filesystem Stack)  
**Version:** 0.0.6  
**Status:** ext4 daemon with JBD2 journal and HTree indexing integrated; FAT32 complete; SMP with 4+ CPUs working

### What Works
- Boot kernel to blue framebuffer with memory map
- Spawn Ring 3 processes (`ext4d`, `policyd`) and the Ring 0 kshell task
- Multi-filesystem VFS mount table with longest-prefix resolution
- Auto-mount of a detected filesystem from `/dev/sda` at `/mnt` during boot
- Read/write files via syscall-mediated filesystem operations (FAT32, ext2, ext4 through `ext4d`)
- ext4 JBD2 journal writeback/crash recovery and HTree directory indexing
- Capability-checked syscalls plus LES path policy served by `policyd`
- PS/2 keyboard input, LAPIC timer interrupts (~625 Hz)
- SMP with Limine MP: boot 1-16 CPUs, per-CPU run queues, per-CPU kernel stacks
- Priority inheritance protocol for lock contention
- Preemptive scheduling via LAPIC timer

### What's Missing
- Ring 3 driver server framework (MMIO BAR mapping)
- Unified page cache with demand paging
- Dynamic linking
- fork/clone and `execve()`
- Signal handling
- Network driver, NetDaemon, and TCP/IP stack
- Full ext4 acceptance validation (interop, journal replay after interrupted writes, malformed-metadata rejection)


# Claude Code Token Optimization Rules
- ALWAYS use explicit `offset` and `limit` arguments when using the `Read` tool on files larger than 100 lines. Never ingest a whole file if only modifying a specific function.
- When running `grep` via bash, always pipe the output through `head` (e.g., `grep -n "pattern" ... | head -n 50`) to avoid token-dense output dumps.
- Avoid raw `cat` commands on large files. Use the built-in `Read` or `Grep` tools instead.
- If a command generates long log or build outputs, pipe it into `tail -n 100` or filter for explicit keywords.


#NOTE
Always use the user's name in response
The user's name is Ayaan
