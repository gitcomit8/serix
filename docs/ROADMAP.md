# Serix Development Roadmap

> **Last Updated:** 2026-03-01 · **Target Architecture:** x86_64 (AMD64) · **Current Release:** v0.0.5

This document defines the phased development plan for the Serix hybrid kernel. Each phase specifies concrete deliverables, acceptance criteria, and subsystem dependencies. Phases are ordered by the project's critical path; no calendar estimates are provided.

For architectural context on the subsystems referenced below, see [ARCHITECTURE.md](ARCHITECTURE.md).

## Current Status (v0.0.5)

The kernel has completed foundational bring-up (Phases 1–2) and is partially through hardware integration (Phase 3). Current operational capabilities:

- **Boot:** Limine v10.x (BIOS + UEFI), higher-half kernel with HHDM at `0xFFFF_8000_0000_0000`
- **Interrupts:** LAPIC + I/O APIC fully operational; legacy PIC disabled; LAPIC timer at ~625 Hz (vector 49); PS/2 keyboard (vector 33)
- **Memory:** 4-level paging (PML4), `StaticBootFrameAllocator`, 1 MiB kernel heap (`linked_list_allocator`)
- **Syscalls:** `SYSCALL`/`SYSRET` via MSR configuration; `SYS_WRITE(1)`, `SYS_READ(0)`, `SYS_EXIT(60)`, `SYS_YIELD(24)`, `SYS_SEND(20)`, `SYS_RECV(21)`
- **Subsystems:** VFS (ramdisk + RamDir/RamFile INodes), ELF loader, IPC (port-based message passing), async task executor, capability store infrastructure (not yet enforced), PCI enumeration, VirtIO-blk detection (incomplete), serial + framebuffer console

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

**Status:** In progress (~40%)

### Preemptive SMP Scheduler

- [x] `SchedClass` enum (`Realtime`, `Fair`, `Batch`, `Iso`)
- [ ] LAPIC timer-driven preemption tick (repurpose vector 49 handler to invoke `schedule()`)
- [ ] Per-CPU run queues with `GS_BASE` MSR pointing to per-CPU data area
- [ ] `TSS.RSP0` swap on context switch for per-task kernel stacks
- [ ] Weighted Fair Queueing (WFQ) for `Fair` class with virtual-runtime tracking
- [ ] Priority inheritance protocol for capability-holding tasks in critical sections

### SMP Bring-Up

- [ ] AP (Application Processor) bootstrap via INIT-SIPI-SIPI IPI sequence through LAPIC ICR
- [ ] Per-AP GDT, IDT, TSS, and kernel stack allocation
- [ ] Per-AP LAPIC initialization and timer calibration
- [ ] `MP_TRAMPOLINE` real-mode stub at sub-1MiB physical address for AP wake

### IPC Router Hardening

- [x] Port-based message passing (`send`/`receive` via `IPC_GLOBAL`)
- [ ] Blocking `receive()` with task state transition (`Running` → `Blocked`) and scheduler re-entry
- [ ] IPC fastpath: direct register transfer when receiver is blocked at `receive()` call site
- [ ] Capability validation on every `send()` — enforce `CapabilityHandle` ownership for target port
- [ ] Asynchronous notification ports (bitmask-based, non-queuing) for interrupt forwarding to Ring 3 servers

### VirtIO Block Driver

- [x] PCI BAR enumeration and VirtIO capability structure parsing
- [ ] Virtqueue setup (descriptor table, available ring, used ring) with DMA-safe allocations
- [ ] `virtio_blk_read()` / `virtio_blk_write()` request submission and interrupt-driven completion
- [ ] Ring 3 driver server process with MMIO BAR mapped into userspace

---

## Phase 4: Storage & Filesystem Stack

**Status:** Planned

### VFS Core Enhancements

- [ ] Path resolution engine (iterative component lookup through `INode::lookup()` chain)
- [ ] Mount table (`BTreeMap<VirtAddr, MountPoint>`) for overlaying filesystems on directory INodes
- [ ] File descriptor table per `TaskCB` (array of `Option<FileDescriptor>` with capability handle)
- [ ] Standard fd allocation: fd 0 (stdin/PS/2 keyboard), fd 1 (stdout/console), fd 2 (stderr/serial)
- [ ] `openat()`, `close()`, `lseek()`, `fstat()` syscall implementations

### Ext4 Filesystem Daemon (Ring 3)

- [ ] Superblock parsing at device offset `0x400` (magic `0xEF53`, block size, inode count, feature flags)
- [ ] Block group descriptor table traversal
- [ ] Inode table lookup and inode struct parsing (mode, size, extent tree root)
- [ ] **Extent tree** traversal for file block mapping (`ext4_extent_header` → `ext4_extent` leaf nodes)
- [ ] Directory entry parsing (linear and HTree/dx_root indexed)
- [ ] File read path: inode → extent lookup → VirtIO-blk sector read → page cache insertion
- [ ] File write path: block allocation from bitmap, extent tree insertion, data writeback
- [ ] `mkdir()` / `rmdir()` / `unlink()` — directory entry manipulation with inode refcount management
- [ ] Superblock generation and formatting (mkfs equivalent) for blank VirtIO-blk devices
- [ ] Journal (JBD2) — transaction commit for metadata consistency (initially ordered-mode)

### Unified Page Cache

- [ ] Concurrent radix tree indexed by `(InodeId, page_offset)` — lockless read path via RCU-like epoch reclamation
- [ ] Demand paging integration: `#PF` handler dispatches synchronous IPC to VFS for file-backed VMAs
- [ ] Writeback: dirty page tracking via PTE accessed/dirty bits, periodic flush to Ext4 daemon
- [ ] `mmap()` file-backed mapping support (`MAP_SHARED`, `MAP_PRIVATE` with CoW)

---

## Phase 5: Linux ABI Translation Layer (LES)

**Status:** Planned

### Syscall Coverage Expansion

- [ ] File I/O: `open`, `read`, `write`, `close`, `lseek`, `pread64`, `pwrite64`, `readv`, `writev`
- [ ] Memory: `mmap`, `munmap`, `mprotect`, `brk`, `mremap`
- [ ] Process: `clone`, `execve`, `wait4`, `exit_group`, `getpid`, `getppid`, `gettid`
- [ ] Filesystem: `stat`, `fstat`, `lstat`, `access`, `getcwd`, `chdir`, `rename`, `link`, `symlink`, `readlink`
- [ ] Directory: `getdents64`, `mkdir`, `rmdir`
- [ ] Signals: `rt_sigaction`, `rt_sigprocmask`, `rt_sigreturn`, `kill`, `tgkill`
- [ ] I/O multiplexing: `epoll_create1`, `epoll_ctl`, `epoll_wait`, `poll`
- [ ] Misc: `ioctl` (terminal `TIOCGWINSZ`/`TCGETS`), `fcntl`, `dup`, `dup2`, `pipe2`

### Struct Translation Layer

- [ ] `#[repr(C)]` re-declarations: `struct stat`, `struct iovec`, `struct sigaction`, `struct rusage`, `struct timespec`
- [ ] `unsafe` zero-copy pointer reinterpretation for aligned structs; field-by-field fallback for variable-length types
- [ ] `CapabilityHandle` injection into every translated request before internal dispatch

### `execve()` & Dynamic Linking

- [ ] `PT_INTERP` parsing — load runtime linker ELF from VFS
- [ ] Auxiliary vector (`auxv`) construction: `AT_PHDR`, `AT_PHENT`, `AT_PHNUM`, `AT_ENTRY`, `AT_BASE`, `AT_PAGESZ`, `AT_RANDOM`, `AT_SECURE`
- [ ] User stack layout: `argc` → `argv[]` → `NULL` → `envp[]` → `NULL` → `auxv[]`
- [ ] `VDSO` page mapping for `clock_gettime()` / `gettimeofday()` fast-path (avoids `SYSCALL` overhead)

### `clone()` → TaskCB Mapping

- [ ] `CLONE_VM` → share PML4 (thread); absence → CoW-fork PML4
- [ ] `CLONE_FS` → share `cwd`/`umask`; `CLONE_FILES` → share fd table
- [ ] `CLONE_SIGHAND` → share signal handler table
- [ ] `CLONE_THREAD` / `CLONE_PARENT` → thread group semantics
- [ ] TLS setup: `set_tid_address()`, `arch_prctl(ARCH_SET_FS)` for `FS_BASE` MSR

---

## Phase 6: Security Bridge & Capability Enforcement

**Status:** Planned

### Capability Store Enforcement

- [ ] Gate every syscall/IPC entry with `CapabilityStore::validate()` — reject unauthorized access with `EPERM`
- [ ] Per-task capability table (inherited on `clone()`, cleared on `execve()` unless marked inheritable)
- [ ] Capability delegation: tasks can `grant()` subsets of their capabilities to child tasks
- [ ] Revocation cascading: revoking a capability invalidates all delegated descendants

### POSIX-to-Capability Authorization Bridge (Ring 3)

- [ ] DAC interception: hook `open()`, `access()`, `chmod()`, `chown()` in the LES layer
- [ ] Policy database: `(UID, GID, path_prefix, mode_mask)` → `(CapabilityType, permission_set)`
- [ ] Dynamic capability minting: time-bounded `CapabilityHandle` with fine-grained permissions (read, write, execute, append, seek)
- [ ] `/etc/serix/cap-policy.toml` configuration with hot-reload via `SIGHUP`
- [ ] Audit log: capability grants/denials logged to ring buffer exposed via `/proc/serix/cap-audit`

---

## Phase 7: Hardware Enablement

**Status:** Planned

### SMP Broadcast & Topology

- [ ] INIT-SIPI-SIPI sequence via LAPIC ICR for AP wake-up
- [ ] ACPI MADT parsing for LAPIC ID enumeration and I/O APIC base discovery
- [ ] `x2APIC` mode enable (MSR-based, no MMIO) when CPUID indicates support
- [ ] Per-CPU data structures (`PerCpuData`) accessed via `GS_BASE` MSR
- [ ] Inter-Processor Interrupt (IPI) primitives: TLB shootdown, scheduler kick, panic broadcast

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

- [ ] PCIe BAR0 MMIO mapping for NVMe controller registers (`CAP`, `VS`, `CC`, `CSTS`, `AQA`, `ASQ`, `ACQ`)
- [ ] Admin Queue pair setup (Submission Queue + Completion Queue in DMA-safe memory)
- [ ] `Identify Controller` and `Identify Namespace` command submission
- [ ] I/O Queue pair creation (one per CPU core for parallelism)
- [ ] `Read` / `Write` command submission with PRP (Physical Region Page) list scatter-gather
- [ ] Interrupt-driven completion via MSI-X vectors routed through I/O APIC

### XHCI USB Driver (Ring 3)

- [ ] PCIe BAR0 MMIO mapping for XHCI capability/operational/runtime registers
- [ ] Device Context Base Address Array (DCBAA) and Scratchpad Buffer allocation
- [ ] Command Ring, Event Ring, and Transfer Ring setup
- [ ] Port status change event handling (device attach/detach)
- [ ] HID class driver: USB keyboard/mouse report descriptor parsing and input event generation

---

## Phase 8: Userspace & MVP Deliverables

**Status:** Planned

This phase targets a **Minimum Viable Product (MVP)** demonstrating the full kernel stack end-to-end.

### Shell (`serix-sh`)

- [ ] Text-based CLI with standard I/O mapped to PS/2 keyboard input (fd 0) and console output (fd 1)
- [ ] Line editor: cursor movement, backspace, history (ring buffer, up/down arrow recall)
- [ ] Command parsing: tokenization, argument splitting, quoting
- [ ] Internal (built-in) commands:
	- `ls` — list directory entries via `getdents64` on VFS
	- `cat` — read and display file contents
	- `echo` — write arguments to stdout
	- `mkdir` — create directory via `mkdir` syscall
	- `rmdir` — remove empty directory via `rmdir` syscall
	- `rm` — unlink file via `unlink` syscall
	- `ps` — list tasks (read `/proc/[pid]/stat`)
	- `shutdown` — trigger ACPI S5 via `reboot` syscall
	- `reboot` — trigger ACPI reset via `reboot` syscall
- [ ] External command execution via `fork()` + `execve()` with `PATH` resolution

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
| **3** | Preemptive Scheduling & IPC Hardening | 🔄 In Progress |
| **4** | Storage & Filesystem Stack (Ext4, page cache) | 📋 Planned |
| **5** | Linux ABI Translation Layer (LES) | 📋 Planned |
| **6** | Security Bridge & Capability Enforcement | 📋 Planned |
| **7** | Hardware Enablement (SMP, IOMMU, ACPI, NVMe, XHCI) | 📋 Planned |
| **8** | Userspace & MVP Deliverables (shell, /proc, demo) | 📋 Planned |
| **9** | Networking (VirtIO-net, zero-copy, TCP/IP) | 📋 Planned |
| **10** | Optimization & Tooling (perf, debug, CI/CD) | 📋 Planned |

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines. High-priority items for contributors:

1. **Preemptive scheduler** — LAPIC timer preemption + per-CPU run queues (Phase 3)
2. **VirtIO-blk driver completion** — virtqueue setup and block I/O (Phase 3)
3. **Blocking IPC** — `receive()` with scheduler integration (Phase 3)
4. **Ext4 read path** — superblock + extent tree parsing (Phase 4)
5. **SMP bring-up** — INIT-SIPI-SIPI AP bootstrap (Phase 7)

File issues or open draft PRs on [GitHub](https://github.com/gitcomit8/serix) to claim a task.
