# Serix Development Roadmap

> **Last Updated:** 2026-11-04 · **Target Architecture:** x86_64 (AMD64) · **Current Release:** v0.0.8

This document defines the phased development plan for the Serix hybrid kernel. Each phase specifies concrete deliverables, acceptance criteria, and subsystem dependencies. Phases are ordered by the project's critical path; no calendar estimates are provided.

For architectural context on the subsystems referenced below, see [ARCHITECTURE.md](ARCHITECTURE.md).

## Current Status (v0.0.8)

The kernel has completed Phases 1–2 and the four core features of Phase 3. Current operational capabilities:

- **Boot:** Limine v10.x (BIOS + UEFI), higher-half kernel with HHDM at `0xFFFF_8000_0000_0000`
- **Interrupts:** LAPIC + I/O APIC fully operational; legacy PIC disabled; LAPIC timer at ~625 Hz (vector 49); PS/2 keyboard (vector 33); VirtIO block (vector 34, IRQ 11)
- **Memory:** 4-level paging (PML4), `StaticBootFrameAllocator`, 1 MiB kernel heap (`linked_list_allocator`), SLUB allocator for large objects and 1 MiB task stacks (`0xFFFF_D000_0000_0000` VA range)
- **Scheduling:** Preemptive round-robin; LAPIC timer invokes `schedule()` at ~625 Hz; `TaskCB` with SLUB-allocated stacks; callee-saved GPR + CR3 context switch; `block_current_and_switch()` for blocking primitives
- **Syscalls (25 total):** `SYSCALL`/`SYSRET` via MSR; Serix-native numbers (not Linux-compatible): Process group — `SYS_EXIT(0)`, `SYS_YIELD(1)`, `SYS_GETPID(2)`, `SYS_GETPPID(3)`, `SYS_SPAWN(4)`, `SYS_WAIT(5)`; File I/O — `SYS_OPEN(10)`, `SYS_CLOSE(11)`, `SYS_READ(12)`, `SYS_WRITE(13)`, `SYS_SEEK(14)`, `SYS_DUP(15)`, `SYS_DUP2(16)`, `SYS_PIPE(17)`, `SYS_GETDENTS(18)`; Filesystem — `SYS_MKDIR(20)`, `SYS_UNLINK(21)`, `SYS_RMDIR(22)`, `SYS_FORMAT(23)`; IPC — `SYS_SEND(30)`, `SYS_RECV(31)`, `SYS_RECV_BLOCK(32)`, `SYS_CREATE_PORT(33)`, `SYS_CREATE_NOTIFICATION_PORT(34)`, `SYS_NOTIFY(35)`; Memory — `SYS_MMAP(90)`, `SYS_MUNMAP(91)`; File read — `SYS_FILE_READ(92)`; Policy — `SYS_POLICY_RELOAD(93)`. Every syscall that touches a resource validates capability authority via the `CapabilityStore` (see Phase 6 below).
- **IPC:** Port-based message passing; blocking `receive_blocking()` with wait queues; `send()` wakes blocked receivers; producer/consumer validated
- **Storage:** VirtIO 1.0 block device (PCI modern, two-phase init); virtqueue with DMA-safe HHDM frame allocation; interrupt-driven sector read/write (IRQ via IOAPIC); `BlockDevice` VFS INode for byte-oriented access; 32 MiB disk, write→read verified
- **Filesystem:** FAT32 driver (`fs/` crate) with BPB parsing, cluster chain traversal/allocation, directory entry creation (8.3 + LFN), file read/write, `mkdir`, `unlink` (with LFN cleanup), duplicate filename rejection, LAPIC-tick timestamps; 32 MiB disk formatted via `mkfs.vfat -F 32`; files created by Serix are visible when mounting `disk.img` on Linux
- **Ext4 (Ring 3):** ext4 daemon (`ext4d`) + kernel VFS IPC stubs are wired into boot, with superblock/BGDT/inode/extent support and IPC-backed `lookup`, `readdir`, `read`, `write`, `mkdir`, `create`, `unlink`
- **File Descriptors:** Global FD table (`kernel/src/fd.rs`) keyed by `(task_id, fd)`; `open()`/`close()`/`seek()` operations; FDs 0-2 backed by stdio INodes (fd 0 → PS/2 keyboard, fd 1 → framebuffer console, fd 2 → serial); user files start at fd 3
- **Subsystems:** VFS (ramdisk + RamDir/RamFile/BlockDevice INodes, longest-prefix mount matching), ELF loader (PT_LOAD, PT_GNU_STACK), IPC, async executor, capability store with per-task cspace and authorization-gated syscalls, LES (Least-Elevated Security) bridge with declarative policy engine + audit ring, PCI enumeration, serial + framebuffer console, fs (FAT32), /proc/serix/cap-audit pseudo-file, VMA manager with SYS_MMAP/MUNMAP

---

## Phase 1: Core Foundation ✅

**Status:** Complete

### Bootloader Integration

- [x] Limine v10.x request/response protocol integration
- [x] Memory map parsing from `MemoryMapRequest` response
- [x] Framebuffer initialization via `FramebufferRequest`
- [x] HHDM offset retrieval via `HhdmRequest`

### Memory Management

- [x] Page table initialization using bootloader-provided CR3
- [x] `StaticBootFrameAllocator` from Limine `USABLE` memory regions
- [x] Heap allocator (1 MiB at `0xFFFF_8000_4444_0000`) via `linked_list_allocator`
- [x] `OffsetPageTable` wrapper for virtual memory manipulation

### Hardware Abstraction Layer

- [x] CPU exception handlers (divide-by-zero, page fault, double fault, GPF)
- [x] APIC bring-up (Local APIC enable, I/O APIC redirection table programming)
- [x] Legacy PIC mask-all and disable
- [x] LAPIC timer driver (~625 Hz periodic, vector 49)
- [x] Serial console (COM1, `0x3F8`, 115200 baud 8N1)

---

## Phase 2: System Infrastructure ✅

**Status:** Complete

### Task Management

- [x] `TaskCB` (Task Control Block) with `TaskId`, `TaskState`, `SchedClass`, `CPUContext`
- [x] Async task creation using Rust `Future` trait objects
- [x] Cooperative round-robin executor (`VecDeque`-based polling loop)
- [x] Low-level `context_switch()` assembly (callee-saved GPRs + CR3 + segment registers)

### Capability System

- [x] 128-bit `CapabilityHandle` generation (`RDTSC`-seeded entropy)
- [x] `CapabilityStore` (`BTreeMap<CapabilityHandle, Capability>`) with `spin::Mutex`
- [x] `CapabilityType` enum: `Task`, `MemoryRegion`, `IODevice`, `FileDescriptor`
- [x] `grant()` / `revoke()` operations

### Syscall Interface

- [x] `SYSCALL`/`SYSRET` MSR configuration (`EFER.SCE`, `LSTAR`, `STAR`, `SFMASK`)
- [x] Naked ASM entry trampoline with kernel stack swap
- [x] Userspace pointer validation (`0x0` – `0x8000_0000_0000` range check)
- [x] `SYS_WRITE`, `SYS_READ`, `SYS_EXIT`, `SYS_YIELD`, `SYS_SEND`, `SYS_RECV` dispatch

### Hardware Detection

- [x] CPUID leaf parsing (vendor string, feature flags, cache topology)
- [x] CPU topology detection (cores, threads, packages)
- [x] Hybrid core classification infrastructure (P-core/E-core via CPUID leaf `0x1A`)

---

## Phase 3: Preemptive Scheduling & IPC Hardening 🔄

**Status:** Core scheduling complete; SMP with Limine MP AP bootstrap operational; WFQ and priority inheritance implemented

### Preemptive SMP Scheduler

- [x] `SchedClass` enum (`Realtime(0-99)`, `Fair(100-139)`, `Batch`, `Iso`)
- [x] LAPIC timer-driven preemption at ~625 Hz invoking `schedule()` (vector 49)
- [x] SLUB-allocated 1 MiB per-task kernel stacks with guard pages
- [x] Callee-saved GPR + CR3 context switch; `block_current_and_switch()` for blocking
- [x] Per-CPU run queues (`PER_CPU_RUN_QUEUES` array, 16 slots); GS_BASE MSR points to per-CPU data area
- [x] Weighted Fair Queueing (WFQ) for `Fair` class with virtual-runtime tracking
- [x] Priority inheritance: `acquire_lock_with_pi()` boosts holder priority; `release_lock_with_pi()` restores original priority; global `LOCK_REGISTRY` tracks lock holders
- [ ] `TSS.RSP0` swap on context switch for per-task kernel stacks (deferred — currently uses `SWITCH_HOOK` callback to update GDT stack pointer)

### IPC Router Hardening

- [x] Port-based message passing (`send`/`receive` via `IPC_GLOBAL`)
- [x] Blocking `receive_blocking()` with `TaskState::Blocked`, per-port wait queues, and scheduler re-entry
- [x] `SYS_RECV_BLOCK (32)` syscall for userspace blocking receive
- [x] `send()` wakes first blocked receiver; producer/consumer validated in QEMU
- [x] Capability validation on every `send()` / `receive()` — `authorize_current_ipc()` resolves capability slot, validates rights against `CapabilityStore`, and extracts port ID before any observable state change
- [x] Async notification ports (`SYS_CREATE_NOTIFICATION_PORT(34)`, `SYS_NOTIFY(35)`) with event bitmask and `check_notification()` integration into `receive_blocking()`
- [x] IPC fastpath: register-mediated direct transfer via `ipc_msg_ptr`/`ipc_msg_valid` in `CPUContext`; context switch loads msg pointer into `rbx` (SysV arg1) after restoring normal registers

### VirtIO Block Driver

- [x] PCI BAR enumeration and VirtIO 1.0 capability structure parsing (COMMON_CFG, NOTIFY_CFG, ISR_CFG, DEVICE_CFG)
- [x] Two-phase init: PCI/feature negotiation before SLUB; virtqueue setup + DRIVER_OK after SLUB
- [x] Virtqueue (descriptor table, available ring, used ring) with DMA-safe HHDM frame allocation
- [x] `read_sector()` / `write_sector()` with polled completion (spin_loop) (IRQ 11 → vector 34, IOAPIC)
- [x] `BlockDevice` VFS INode: byte-oriented read/write with sector-aligned translation and read-modify-write
- [x] 32 MiB disk (65536 sectors), write→read verified via VFS interface
- [ ] Ring 3 driver server process with MMIO BAR mapped into userspace (Phase 7)

---

## Phase 4: Storage & Filesystem Stack

**Status:** In progress. Existing ext4d/cache code is MVP-only until its listed recovery and coherence acceptance tests are added and pass.

### VFS Core Enhancements

- [x] Path resolution engine (iterative component lookup through `INode::lookup()` chain)
- [x] Mount table (`BTreeMap<String, Arc<dyn INode>>` keyed by mount-point path with longest-prefix matching)
- [x] File descriptor table (global table keyed by `(task_id, fd)`, not per-TaskCB)
- [x] Standard fd allocation: fd 0 (stdin/PS/2 keyboard), fd 1 (stdout/console), fd 2 (stderr/serial)
- [x] `SYS_OPEN`, `SYS_CLOSE`, `SYS_SEEK` syscall implementations

### FAT32 Filesystem (Ring 0)

- [x] BPB (BIOS Parameter Block) parsing from sector 0 (bytes_per_sector, sectors_per_cluster, reserved_sectors, fat_count, sectors_per_fat, root_cluster)
- [x] FAT cluster chain traversal (`fat_read_entry`) and allocation (`fat_alloc_cluster` with `fat_write_entry`)
- [x] Directory entry parsing with 8.3 SFN and Long File Name (LFN) support
- [x] Directory entry creation (LFN + 8.3 pair with correct checksum and sequence numbers)
- [x] File read path: cluster chain → sector read → byte-offset copy
- [x] File write path: cluster chain extension, data write, directory entry size update
- [x] VFS integration: `FatDirINode` and `FatFileINode` implementing `vfs::INode` trait
- [x] `mount()` function parsing BPB from VirtIO block device sector 0
- [x] Linux interop: `disk.img` mountable on Linux via `mount -o loop` to inspect files created by Serix

- [x] Duplicate filename rejection in `insert()` (returns error if name already exists)
- [x] `mkdir()` — allocate cluster, write `.`/`..` entries, insert parent directory entry; `SYS_MKDIR(20)`
- [x] `unlink()` — mark SFN entry deleted (`0xE5`), wipe associated LFN entries; `SYS_UNLINK(21)`
- [x] Directory entry timestamps encoded from LAPIC tick counter (creation + modified fields)
- [x] DMA frame reuse: single pre-allocated DMA buffer per `VirtioBlock`; no per-request allocation

### Ext4 Filesystem Daemon (Ring 3)

- [x] Superblock parsing at device offset `0x400` (magic `0xEF53`, block size, inode count, feature flags)
- [x] Block group descriptor table traversal
- [x] Inode table lookup and inode struct parsing (mode, size, extent tree root)
- [x] **Extent tree** traversal for file block mapping (`ext4_extent_header` → `ext4_extent` leaf nodes)
- [ ] Directory entry parsing (linear + HTree/dx_root indexed) — linear entries implemented; HTree pending
- [x] File read path: inode → extent lookup → VirtIO-blk sector read
- [x] File write path: block allocation from bitmap, extent tree insertion, data writeback
- [x] `rmdir()` syscall (`SYS_RMDIR(22)`) — validates METADATA rights on parent, delegates to `inode.rmdir()`
- [ ] Stricter ext4 inode/link-count checks in ext4d daemon
- [x] `SYS_FORMAT(23)` — formats a VirtIO-blk device with ext4 filesystem (block size, inode count derived from device capacity); requires `PLATFORM_ADMIN` right on a block device capability
- [ ] Superblock generation and formatting via CLI from kshell (`format /dev/sda` builtin)
- [ ] Journal (JBD2) — transaction commit for metadata consistency (initially ordered-mode)

### Unified Page Cache

- [x] Concurrent radix tree indexed by `(InodeId, page_offset)` with lockless read path via RCU-like epoch reclamation
- [x] Demand paging integration: `#PF` handler dispatches synchronous IPC to VFS for file-backed VMAs
- [x] Writeback: dirty page tracking via PTE accessed/dirty bits, deferred writeback
- [x] `mmap()` file-backed mapping support (`MAP_SHARED` and `MAP_PRIVATE` with CoW); `cow_fault()` on write fault to private mappings

---

## Phase 5: LES (Least-Elevated Security) Policy Bridge

**Status:** Complete. Policy daemon is spawned at boot with TOML config, hot-reload, and LES bridge fully operational.

> **Clarification:** "LES" in Serix stands for **Least-Elevated Security**, not Linux Executable Subsystem. It is a declarative access control bridge, not a Linux ABI translation layer.

### LES Policy Engine (Implemented)

- [x] `les_bridge` module (`kernel/src/les_bridge.rs`) with atomic `PolicySnapshot` swap for transactional policy reload
- [x] Local deny/allow rules evaluated at syscall entry: default-allow policy, explicit deny paths
- [x] Default deny rules at boot: `/etc/shadow`, `/proc/serix/cap-audit`
- [x] Policy daemon fallback: kernel queries Ring-3 `policyd` (port 0x5000) for paths not covered by local snapshot
- [x] `PolicyRule` type: `(path: &str, action: PolicyAction::Deny | Allow)`
- [x] `SYS_POLICY_RELOAD(93)` syscall for atomic `PolicySnapshot` swap (transactional reload)
- [x] TOML config parser (no_std compatible, used by `policyd`)

### Audit Ring (Implemented)

- [x] Bounded overwrite-on-full audit ring buffer (`capability::audit::AuditRing`, ~6KB)
- [x] `AuditRecord` with caller, operation, target, rights, result
- [x] `/proc/serix/cap-audit` pseudo-file exposes audit log via `procfs::make_audit_file()`
- [x] `init_audit()` called early in `_start()` before any capability operations

### Completed

- [x] Policy daemon (`policyd`) — Ring-3 daemon spawned at boot (alongside ext4d and kshell) with TOML config parsing and hot-reload (`SYS_POLICY_RELOAD(93)`)

### Remaining

- [ ] Audit log pagination and truncation
- [ ] Declarative policy rules for IPC ports and device capabilities

### `clone()` → TaskCB Mapping

- [ ] `CLONE_VM` → share PML4 (thread); absence → CoW-fork PML4
- [ ] `CLONE_FS` → share `cwd`/`umask`; `CLONE_FILES` → share fd table
- [ ] `CLONE_SIGHAND` → share signal handler table
- [ ] `CLONE_THREAD` / `CLONE_PARENT` → thread group semantics
- [ ] TLS setup: `set_tid_address()`, `arch_prctl(ARCH_SET_FS)` for `FS_BASE` MSR

---

## Phase 6: Security Bridge & Capability Enforcement

**Status:** Core enforcement operational; delegation and advanced features in progress

### Capability Store Enforcement (Implemented)

- [x] Every resource-accessing syscall validates capability authority before observable state changes:
  - `SYS_OPEN`, `SYS_READ`, `SYS_WRITE`, `SYS_SEEK`, `SYS_CLOSE`, `SYS_DUP`, `SYS_DUP2` → inode authority
  - `SYS_SEND`, `SYS_RECV`, `SYS_RECV_BLOCK`, `SYS_CREATE_PORT` → IPC port authority
  - `SYS_MKDIR`, `SYS_UNLINK`, `SYS_RMDIR` → parent directory METADATA authority
  - `SYS_PIPE` → directory CONTROL authority
  - `SYS_MMAP`, `SYS_MUNMAP` → inode MMAP/MUNMAP authority
  - `SYS_FORMAT` → block device PLATFORM_ADMIN authority
  - `SYS_EXIT`, `SYS_YIELD`, `SYS_GETPID`, `SYS_GETPPID` → no authority required
- [x] Per-task capability space (`TaskCB.cspace`: `Arc<Mutex<CapabilitySpace>>`) with slot generation tracking
- [x] Capability store (`CapabilityStore`: `BTreeMap<CapabilityId, CapabilityRecord>`) with `mint_root()`, `delegate()`, `revoke()`
- [x] `authorize_current_inode()` / `authorize_current_ipc()` central authorization helpers
- [x] Revocation cascading: revoking a capability invalidates all delegated descendants
- [x] Stale generation rejection: reused slots get new generations, stale handles are rejected before store lookup
- [x] Validation epoch tracking: `delegate_checked()` requires `acknowledge_validation_epoch()` to prevent TOCTOU
- [x] Secure entropy: `RDRAND`/`RDSEED` health-checked during boot; restricted mode when unavailable
- [x] `CapabilityRef { slot, generation }` — 128-bit handles decomposed into slot + generation for validation

### Planned Extensions

- [ ] POSIX DAC bridge: UID/GID/mode semantics translated into capability tickets
- [ ] `/etc/serix/cap-policy.toml` with hot-reload via `SYS_POLICY_RELOAD(93)`

---

## Phase 7: Hardware Enablement

**Status:** Basic SMP operational; ACPI table discovery/parsing added; firmware-driven APIC routing, IOMMU, power controls, and isolated device servers remain in progress

### SMP Broadcast & Topology

- [x] Per-CPU data structures (`PerCpuData`) accessed via `GS_BASE` MSR; `PER_CPU_DATA_BASE` set during init
- [x] Per-CPU run queues; `current_cpu_id()` reads LAPIC ID from MSR `0x1B`
- [x] Limine MP callback-based AP bootstrap; BSP signals `AP_READY_PTR` on completion
- [ ] INIT-SIPI-SIPI sequence via LAPIC ICR for AP wake-up (currently relies on Limine to hand off running APs)
- [ ] ACPI MADT parsing for LAPIC ID enumeration and I/O APIC base discovery
- [x] ACPI RSDP/root/SDT validation and MADT/DMAR/FADT decoding (discovery diagnostics; not yet wired into APIC routing or controllers)
- [ ] `x2APIC` mode enable (MSR-based, no MMIO) when CPUID indicates support
- [x] xAPIC/x2APIC register access selection, APIC ID reads, and fixed IPI send primitive (build-checked; QEMU boot validation pending)
- [ ] Inter-Processor Interrupt (IPI) primitives: TLB shootdown, scheduler kick, panic broadcast
- [x] Reserved TLB/scheduler/panic IPI vectors with per-CPU mailboxes and acknowledgement timeout API (not yet connected to VMA reclamation or run-queue wakeups)
- [ ] BORE scheduler policy, only after SMP, per-CPU scheduling, and IPI support have stabilized; WFQ remains the Phase 3 baseline.

### IOMMU (Intel VT-d / AMD-Vi)

- [ ] ACPI **DMAR** table parsing (DMA Remapping Hardware Unit Definition structures)
- [ ] IOMMU page table construction (4-level, analogous to CPU paging)
- [ ] Per-device DMA domain isolation — restrict each PCIe function's DMA to allocated frame ranges
- [ ] Interrupt remapping via IOMMU Interrupt Remapping Table (IRT) to prevent MSI injection attacks
- [ ] Fault logging: IOMMU fault events surfaced to Server Manager via IPC

### Power Management

- [ ] **ACPI FADT** parsing: `PM1a_CNT_BLK` for S5 (shutdown), `RESET_REG` for reboot
- [ ] **C-States:** `MWAIT` instruction with target C-state hint (CPUID leaf `0x05` for supported sub-states); idle loop transitions from `HLT` to `MWAIT`-based
- [ ] **P-States (Intel HWP):**
	- Enable HWP via `IA32_PM_ENABLE` (MSR `0x770`)
	- Configure `IA32_HWP_REQUEST` (MSR `0x774`): set `Minimum_Performance`, `Maximum_Performance`, `Desired_Performance`, `Energy_Performance_Preference`
	- Read `IA32_HWP_CAPABILITIES` (MSR `0x771`) for hardware performance bounds
- [ ] **Thermal monitoring:** `IA32_THERM_STATUS` MSR polling; throttle scheduler on thermal trip

### NVMe Storage Driver (Ring 3)

- [x] PCI inventory exposes class/subclass/prog-if classification, safe BAR address decoding, and bounded MSI-X metadata discovery (no assignment, enablement, or Ring 3 handoff yet)
- [ ] PCIe BAR0 MMIO mapping for NVMe controller registers (`CAP`, `VS`, `CC`, `CSTS`, `AQA`, `ASQ`, `ACQ`)
- [ ] Admin Queue pair setup (Submission Queue + Completion Queue in DMA-safe memory)
- [ ] `Identify Controller` and `Identify Namespace` command submission
- [ ] I/O Queue pair creation (one per CPU core for parallelism)
- [ ] `Read` / `Write` command submission with PRP (Physical Region Page) list scatter-gather
- [ ] Interrupt-driven completion via MSI-X vectors routed through I/O APIC

### XHCI USB Driver (Ring 3)

- [x] PCI inventory classifies xHCI and can decode MSI-X layout metadata (controller driver/server remains unimplemented)
- [ ] PCIe BAR0 MMIO mapping for XHCI capability/operational/runtime registers
- [ ] Device Context Base Address Array (DCBAA) and Scratchpad Buffer allocation
- [ ] Command Ring, Event Ring, and Transfer Ring setup
- [ ] Port status change event handling (device attach/detach)
- [ ] HID class driver: USB keyboard/mouse report descriptor parsing and input event generation

---

## Phase 8: Userspace & MVP Deliverables

**Status:** Planned

This phase targets a **Minimum Viable Product (MVP)** demonstrating the full kernel stack end-to-end.

### Userspace Shell (`rsh`)

> **Note:** Serix currently has `kshell`, a kernel-resident Ring-0 shell with commands: `help`, `echo`, `ls`, `cat`, `write`, `mkdir`, `rm`, `mount`, `umount`, `halt`, `reboot`, output redirection (`>` and `>>`). `rsh` is a planned Ring-3 userspace shell.

Existing Rust shell to be ported from `std` to `#![no_std]` + `ulib` for Serix userspace.

**Port from std to no_std:**

- [ ] Replace `std::io` stdin/stdout with `ulib::read(STDIN)` / `ulib::write(STDOUT)` syscall wrappers
- [ ] Replace `HashMap` → `BTreeMap` (from `alloc`), keep `Vec`/`String` via `alloc`
- [ ] Replace `std::process::exit()` → `ulib::exit()`
- [ ] Add userspace heap allocator (bump allocator or `linked_list_allocator` over `brk`/fixed region)
- [ ] Remove ANSI escape sequences that depend on terminal emulation (adapt for framebuffer console)
- [ ] Build as `#![no_std]` `#![no_main]` binary linked with `user.ld`

**Filesystem builtins (require working VFS syscalls):**

- [ ] `ls` — list directory entries via `SYS_GETDENTS` or VFS directory read
- [ ] `cat` — `serix_open()` + `read()` + `write(STDOUT)` + `serix_close()`
- [ ] `mkdir` — create directory via `SYS_MKDIR`
- [ ] `rm` — unlink file via `SYS_UNLINK`
- [ ] `touch` — create empty file via `serix_open()` + `serix_close()`
- [ ] `pwd` / `cd` — working directory tracking (requires `SYS_CHDIR`/`SYS_GETCWD` or client-side state)

**Process builtins (require clone/execve/waitpid syscalls):**

- [ ] `ps` — list tasks (read `/proc/[pid]/stat` or dedicated `SYS_TASKINFO`)
- [ ] `shutdown` / `reboot` — trigger ACPI S5/reset via `SYS_REBOOT`
- [ ] External command execution via `fork()` + `execve()` with `PATH` resolution

**Existing features to preserve:**

- [x] REPL loop with prompt
- [x] Tokenizer with quoted strings, escape sequences, variable substitution
- [x] Shell variables (`set`/`get`)
- [x] Control flow (`if`, `repeat`)
- [x] Command history
- [ ] Line editor: cursor movement, backspace, arrow-key history recall

### Synthetic `/proc` Pseudo-Filesystem

- [ ] `/proc/meminfo` — frame allocator statistics: total frames, free frames, used frames, page cache occupancy
- [ ] `/proc/stat` — per-CPU idle time accumulators (ticks spent in `MWAIT`/`HLT` idle loop vs. task execution)
- [ ] `/proc/cpuinfo` — CPUID-derived model name, frequency, core type (P-core/E-core), cache sizes
- [ ] `/proc/[pid]/stat` — per-task: state, CPU time (user + system ticks), scheduling class, priority
- [ ] `/proc/[pid]/maps` — per-task VMA listing (start, end, permissions, backing INode)
- [ ] `/proc/uptime` — system uptime derived from LAPIC timer tick count

### `intro` — Architecture Demonstration Binary

- [ ] Spawns N worker threads via `clone(CLONE_VM | CLONE_FS | CLONE_FILES)` to validate thread semantics
- [ ] Each worker performs a configurable compute-bound workload (e.g., matrix multiply, memory streaming)
- [ ] Reads architectural **PMU counters** (Performance Monitoring Unit) via `rdpmc` instruction:
	- `IA32_FIXED_CTR1` (unhalted core cycles) and `IA32_FIXED_CTR2` (unhalted reference cycles) for frequency estimation
	- `IA32_PERFEVTSEL0` programmed for LLC cache miss events (`event=0x2E`, `umask=0x41`)
- [ ] Displays **cache warmth tracking**: per-thread L1d/L2/LLC hit rates before and after core migration
- [ ] Measures and displays **context switch latency**: two threads ping-pong via IPC; timestamp delta via `RDTSC` with invariant TSC calibration
- [ ] **Acceptance criterion:** ≤ 500 ns context switch latency on P-cores (measured as 99th percentile over 10,000 iterations)

---

## Phase 9: Networking

**Status:** Planned

### VirtIO-net Driver (Ring 3)

- [ ] Virtqueue setup for TX and RX (separate queue pairs)
- [ ] MAC address read from VirtIO device configuration space
- [ ] RX: post buffer descriptors to available ring; process incoming frames from used ring
- [ ] TX: construct frame in descriptor buffer; submit to available ring; poll/interrupt for completion
- [ ] DMA buffer registration in IOMMU before driver process start

### Zero-Copy Network Buffer Architecture

- [ ] `CapabilityHandle` grant from network driver to application for shared TX/RX buffer region
- [ ] Application-side `mmap()` of shared buffer into process VMA
- [ ] Scatter-gather descriptor management: application fills TX descriptors in-place; driver submits to virtqueue
- [ ] RX zero-copy: DMA deposits frame directly into application-mapped buffer; notification via event channel

### TCP/IP Stack (Userspace Library)

- [ ] Integration of `smoltcp` (or equivalent `#[no_std]` Rust TCP/IP library) as a userspace linkable crate
- [ ] Socket API shim: `socket()`, `bind()`, `listen()`, `accept()`, `connect()`, `send()`, `recv()`
- [ ] ARP table management, DHCP client for dynamic IP configuration
- [ ] Loopback interface for local IPC testing

---

## Phase 10: Optimization & Tooling

**Status:** Planned

### Scheduler Optimization

- [ ] Cache warmth heuristic: track `last_run_cpu` per `TaskCB`; apply migration penalty in WFQ virtual-runtime calculation
- [ ] NUMA-aware frame allocation: prefer frames from the NUMA node local to the scheduling CPU
- [ ] `XSAVE`/`XRSTOR` lazy FPU context switching: defer FPU state save until another task on the same CPU uses FPU

### Debugging Infrastructure

- [ ] GDB stub (`serix-dbg`): RSP (Remote Serial Protocol) over serial; register read/write, memory read, breakpoints
- [ ] Kernel panic handler: unwind stack via `.eh_frame`, resolve addresses to symbols via embedded symbol table
- [ ] `kdump`: on panic, snapshot kernel state to reserved memory region; Server Manager writes dump to Ext4 on next boot
- [ ] `/proc/serix/trace` — lightweight ring buffer tracing (syscall entry/exit, context switches, IPC sends)

### CI/CD Pipeline

- [ ] GitHub Actions workflow: `cargo build --release`, `cargo clippy`, `cargo fmt --check`
- [ ] Automated QEMU boot test: `make run` with timeout, grep serial output for `[CHECKPOINT]` markers
- [ ] `Miri` undefined behavior checks on `unsafe`-heavy crates (`memory/`, `task/`, `kernel/`)
- [ ] `Kani` bounded model checking for critical invariants (capability store, IPC port queue bounds)

---

## Phase Summary

| Phase | Description | Status |
|---|---|---|
| **1** | Core Foundation (boot, memory, HAL) | ✅ Complete |
| **2** | System Infrastructure (tasks, capabilities, syscalls) | ✅ Complete |
| **3** | Preemptive Scheduling & IPC Hardening | ✅ Core complete; SMP with Limine MP, WFQ, priority inheritance |
| **4** | Storage & Filesystem Stack (FAT32, ext4, mount table) | ✅ FAT32 complete; ext4 daemon MVP wired; rmdir + format added; mount table implemented |
| **5** | LES (Least-Elevated Security) Policy Bridge | ✅ Policy engine + audit ring implemented; daemon integration in progress |
| **6** | Security Bridge & Capability Enforcement | ✅ Core enforcement operational (19 syscalls gated); delegation/inheritance planned |
| **7** | Hardware Enablement (SMP, IOMMU, ACPI, NVMe, XHCI) | 🔄 Basic SMP operational (per-CPU data, Limine MP bootstrap); IOMMU/ACPI pending |
| **8** | Userspace & MVP Deliverables (rsh shell, /proc, demo) | 📋 Planned (kshell Ring-0 shell exists with 11 commands) |
| **9** | Networking (VirtIO-net, zero-copy, TCP/IP) | 📋 Planned |
| **10** | Optimization & Tooling (perf, debug, CI/CD) | 📋 Planned |

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines. High-priority items for contributors:

1. **Shell (`rsh`) no_std port** — port [rsh](https://github.com/gitcomit8/rsh) to `#![no_std]` + `ulib` (Phase 8)
2. **Ext4 daemon hardening** — HTree directories, `rmdir` semantics, and journaling (Phase 4/7)
3. **Capability enforcement** — gate syscalls/IPC on `CapabilityHandle` (Phase 6)
4. **SMP bring-up** — INIT-SIPI-SIPI AP bootstrap + per-CPU run queues (Phase 7)

File issues or open draft PRs on [GitHub](https://github.com/gitcomit8/serix) to claim a task.
