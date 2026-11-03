# Serix Kernel Architecture

> **Target ISA:** x86_64 (AMD64) · **Privilege Model:** Ring 0 / Ring 3 Hybrid · **Current Release:** v0.0.6

## Table of Contents

- [1. Executive Summary](#1-executive-summary)
- [2. Hybrid Kernel Ring Model](#2-hybrid-kernel-ring-model)
- [3. Linux Executable Subsystem (LES) — ABI Translation Layer](#3-linux-executable-subsystem-les--abi-translation-layer)
- [4. Memory Management Subsystem](#4-memory-management-subsystem)
- [5. IPC Router](#5-ipc-router)
- [6. Scheduler](#6-scheduler)
- [7. Hardware & Driver Ecosystem](#7-hardware--driver-ecosystem)
- [8. Zero-Copy Networking Stack](#8-zero-copy-networking-stack)
- [9. Security Model — POSIX-to-Capability Bridge](#9-security-model--posix-to-capability-bridge)
- [10. Initialization — Server Manager (PID 1)](#10-initialization--server-manager-pid-1)
- [11. Subsystem Documentation Index](#11-subsystem-documentation-index)
- [12. Crate Map](#12-crate-map)

## 1. Executive Summary

Serix is a **hybrid-kernel** operating system written in `#![no_std]` Rust for the x86_64 architecture. The kernel enforces a **capability-based security model** and provides a **Least-Elevated Security (LES)** bridge — a declarative policy engine that gates resource access on cryptographic capabilities.

- The architecture pins latency-critical subsystems (SMP scheduler, VFS, Memory Manager, IPC Router) in **Ring 0** for deterministic dispatch.
- Crash-prone and high-latency subsystems (device drivers, filesystem daemons, network stack) are isolated as **Ring 3 userspace servers** communicating via synchronous/asynchronous IPC.
- The **LES bridge** evaluates a local deny/allow policy at syscall entry; for paths not covered by the local snapshot, the kernel queries a Ring-3 policy daemon (`policyd`).
- A **capability-based access control system** with per-task cspace, delegation, revocation cascading, and audit logging ensures no resource access bypasses the `CapabilityStore`.

Serix provides its own **Serix-native syscall ABI** (not a Linux ABI translation layer). Userspace binaries are compiled with `#![no_std]` against the `ulib` crate.

## 2. Hybrid Kernel Ring Model

The kernel partitions subsystems across two privilege rings based on latency sensitivity and fault isolation requirements.

### 2.1 Ring 0 — Kernel-Resident Subsystems

These modules execute in supervisor mode with direct access to CR3, MSRs, and MMIO-mapped hardware registers. They are statically linked into the kernel binary and loaded by Limine at boot.

| Subsystem | Crate | Rationale |
|---|---|---|
| **SMP Scheduler** | `task/` | Sub-microsecond dispatch; direct access to per-CPU `TSS`, `GS_BASE` MSR, and LAPIC timer |
| **Memory Manager** | `memory/` | Owns PML4 hierarchy; handles `#PF` traps synchronously; manages HHDM translations |
| **VFS Core** | `vfs/` | INode dispatch and unified page cache lookups must avoid IPC round-trip overhead |
| **IPC Router** | `ipc/` | Fastpath message copy between `TaskCB` kernel stacks; capability validation inline |
| **Interrupt Dispatcher** | `idt/`, `apic/` | IDT vectors, LAPIC/I/O APIC programming, EOI signaling |
| **Capability Store** | `capability/` | `BTreeMap`-backed capability validation on every syscall/IPC boundary |
| **Syscall Entry** | `kernel/` | `SYSCALL`/`SYSRET` trampoline; MSR configuration (`LSTAR`, `STAR`, `SFMASK`, `EFER.SCE`) |
| **Filesystem Core (FAT32/ext2/ext4 stub)** | `fs/` | In-kernel VFS-facing filesystem drivers and ext4 IPC stubs |

### 2.2 Ring 3 — Userspace Server Processes

These subsystems run as isolated processes in their own address spaces. Faults in any server trigger process-level restart without kernel panic.

| Server | Transport | Description |
|---|---|---|
| **Ext4 Filesystem Daemon** | Synchronous IPC | Current MVP daemon for `lookup/readdir/read/write/mkdir/create/unlink` over ext4 extents |
| **Network Stack** | Zero-copy shared buffers | TCP/IP processing on VirtIO-net ring buffers mapped into application address space |
| **Block Driver (VirtIO-blk)** | Polled I/O | Currently Ring 0; submits I/O descriptors to VirtIO virtqueues; polled completion |
| **USB/Input Driver (XHCI)** | Asynchronous IPC | Manages XHCI host controller rings; delivers HID events via IPC ports |
| **Display Server (GOP)** | Shared framebuffer | Composites client surfaces onto UEFI GOP linear framebuffer |
| **POSIX-to-Capability Bridge** | Synchronous IPC | Translates `UID`/`GID`/`mode` DAC checks into `CapabilityHandle` tickets |
| **Server Manager (PID 1)** | — | Bootstrap orchestrator; constructs dependency DAG; see [§10](#10-initialization--server-manager-pid-1) |

### 2.3 Ring Transition ABI

- **User → Kernel:** `SYSCALL` instruction. RCX/R11 saved by hardware. Kernel entry (`syscall_entry`) performs `swapgs`, saves full context to user stack, switches to kernel stack via `GS_BASE`-referenced per-CPU data area. SFMASK clears `IF` during entry. Context switch uses `SWITCH_HOOK` callback to update GDT stack pointer (not `TSS.RSP0`).
- **Kernel → User:** `SYSRET` restores RCX → RIP, R11 → RFLAGS. For full context restores (e.g., `enter_user_mode` / `iretq`), `IRETQ` is used instead.
- **IPC Fastpath (register-mediated):** When `send()` injects a direct message, the message pointer is stored in the receiver's `TaskCB.direct_msg` (atomic) and `ipc_msg_ptr`/`ipc_msg_valid` fields in `CPUContext`. During `context_switch()`, after restoring normal registers, the kernel checks `ipc_msg_valid`; if set, it loads `ipc_msg_ptr` into `rbx` (SysV arg1) and `r12`/`r13` (arg2/arg3). The receiver's saved `CPUContext` sees the IPC data in callee-saved registers — a zero-copy register-mediated transfer, not a buffered copy. The receiver consumes it via `consume_direct_message()`.

## 3. LES (Least-Elevated Security) — Declarative Policy Bridge

The LES provides a declarative access control mechanism. At boot, the kernel loads a small set of deny rules. At syscall entry, resource operations are checked against the local policy snapshot; if no local rule matches, the kernel queries a Ring-3 policy daemon (`policyd`) for a decision.

### 3.1 Policy Engine Architecture

- **Local Policy Snapshot** — `PolicySnapshot` (a `Vec<PolicyRule>`) loaded atomically via `AtomicUsize` pointer swap. Evaluates deny/allow rules for a `(path, rights)` pair. Default-allow: paths not explicitly denied are allowed.
- **Policy Daemon Fallback** — When the local snapshot has no matching rule, the kernel sends a `MSG_QUERY` to `policyd` on IPC port `0x5000`. The daemon responds on a per-task reply port. Default allow if daemon is unreachable (fail-open).
- **Transactional Reload** — `set_policy()` atomically swaps the snapshot pointer; old snapshot is dropped. Running syscalls holding a reference to the old snapshot are unaffected.

### 3.2 Default Deny Rules (at boot)

- Deny read access to `/etc/shadow`
- Deny access to `/proc/serix/cap-audit` (audit log — admin only)

### 3.3 Audit Ring

- Bounded overwrite-on-full ring buffer (`~6KB`) in kernel memory
- Each `AuditRecord` captures: caller task ID, operation, target, rights, result (granted/denied)
- Exposed via `/proc/serix/cap-audit` pseudo-file (`procfs::make_audit_file()`)

### 3.4 Syscall Entry Trampoline

- The `SYSCALL` entry point (`kernel/src/syscall.rs`) is a **zero-overhead naked function** (`#[unsafe(naked)]`) emitting raw assembly via `naked_asm!`.
- Register state: `RAX`=syscall number, `RDI`/`RSI`/`RDX`/`R10`/`R8`/`R9`=arguments.
- Serix-native syscall numbers (not Linux-compatible): Process group (0–5), File I/O (10–18), Filesystem (20–23), IPC (30–35), Memory (90–91).
- Every resource-accessing syscall validates capability authority via `authorize_current_inode()` or `authorize_current_ipc()` before dispatching to the handler.

### 3.5 ELF Loading & Process Bootstrapping

- The ELF loader (`loader/`) parses `PT_LOAD` and `PT_GNU_STACK` program headers.
- `PT_INTERP` is **not yet implemented** — all user binaries are statically linked.
- Process spawning: `process::spawn_user_process()` allocates a PML4, maps `PT_LOAD` segments at `0x200000` (via `user.ld` linker script), constructs a 16 KiB user stack, and enters Ring 3 via `iretq`.
- **No auxiliary vector, no dynamic linking, no `execve()`** — these are planned (Phase 5+).

## 4. Memory Management Subsystem

### 4.1 Address Space Layout

| Region | Virtual Address Range | Description |
|---|---|---|
| **Userspace** | `0x0000_0000_0000_0000` – `0x0000_7FFF_FFFF_FFFF` | Per-process; isolated via CR3 swap |
| **Non-canonical hole** | `0x0000_8000_0000_0000` – `0xFFFF_7FFF_FFFF_FFFF` | Hardware-enforced gap (`#GP` on access) |
| **HHDM (Physical RAM)** | `0xFFFF_8000_0000_0000` + offset | Direct map of all physical memory; offset from Limine `HhdmRequest` |
| **Kernel Heap** | `0xFFFF_8000_4444_0000` – `0xFFFF_8000_4454_0000` | 1 MiB; `linked_list_allocator`; growable via frame allocator |
| **Kernel Code/Data** | Upper canonical half (Limine-assigned) | Higher-half kernel; PML4 entries 256–511 shared across all address spaces |

### 4.2 VMA Manager and `mmap`

- **VMA Manager (per-process):** Each process maintains its own `VmaManager` (in `memory::vma`). VMAs track start/end address, permissions, `MappingType` (`Private` or `Shared`), and optional `backing_inode` for CoW/demand paging. The `#PF` handler receives the per-process `VmaManager` reference to dispatch faults.
- `SYS_MMAP` — Maps a file into userspace: creates a per-process VMA with `MappingType` (`MAP_SHARED` or `MAP_PRIVATE`). For `MAP_PRIVATE`, pages are demand-loaded on fault as private writable copies (CoW semantics). For `MAP_SHARED`, pages are demand-loaded from the backing inode on `#PF`.
- `SYS_MUNMAP` — Removes a VMA by matching the faulting address and verifying Arc-pointer identity against the stored capability pointer.
- **Demand paging** — `#PF` handler (`handle_page_fault()` in `memory/src/vma.rs`) dispatches page faults: for file-backed VMAs, it reads pages from the backing inode on demand.
- **Copy-on-write** — `cow_fault()` allocates a new private frame, reads the original page from the backing inode, and maps it writable on write faults to `MAP_PRIVATE` VMAs.
- **Deferred page cache writeback** — dirty pages are tracked and written back lazily.

### 4.3 Frame Allocator

- **Boot Phase:** `StaticBootFrameAllocator` pre-populates a frame list from the Limine memory map (`MemoryMapEntryType::USABLE` regions).
- **No deallocation:** Frames cannot be freed; the allocator only supports allocation. This is a known limitation.
- **SLUB allocator:** Used for large kernel allocations and task stacks. 1 MiB per-task stacks allocated from `0xFFFF_D000_0000_0000` VA range via `memory::kstack::alloc_kernel_stack()`.

### 4.4 IOMMU (Not Implemented)

> **Status:** Planned for Phase 7. Not currently implemented.

- Intel VT-d / AMD-Vi support is planned: ACPI DMAR table parsing, IOMMU page table construction, per-device DMA domain isolation.
- Currently, VirtIO-blk DMA buffers use HHDM physical addresses without IOMMU translation.

## 5. IPC Router

### 5.1 Message Passing Model

- **Ports:** Each IPC endpoint is a `Port` — a `Mutex<VecDeque<Message>>` with a configurable depth (default 32).
- **Messages:** Fixed 128-byte payload (`Message { sender_id, id, len, data: [u8; 128] }`).
- **IPC Space:** A global `BTreeMap<PortId, Port>` registry (`IPC_GLOBAL`) for port lookup.
- **Operations:** `send(port_id, msg)` enqueues; `receive(port_id)` dequeues (currently non-blocking; blocking semantics planned via scheduler integration).
- **Blocking Receive:** `receive_blocking()` suspends the calling task (sets `TaskState::Blocked`) until a message arrives; `send()` wakes the first blocked receiver.

### 5.2 IPC Wake-up Path

- `send()` wakes the first blocked receiver on the target port via `wake_task()`, which enqueues the receiver on the per-CPU run queue.
- The blocked receiver (in `receive_blocking()`) may receive a direct-injected message via register-mediated fastpath: `context_switch()` loads `ipc_msg_ptr` into `rbx` (SysV arg1) when `ipc_msg_valid` is set. The receiver sees the IPC pointer in its callee-saved registers before checking the message queue. This is a register-mediated transfer, not a buffered copy. The receiver consumes the message via `consume_direct_message()`.
- Direct register transfer between sender and receiver contexts is **now implemented** via `ipc_msg_ptr`/`ipc_msg_valid` fields in `CPUContext` and context switch assembly in `task/src/context_switch.rs`.

### 5.3 Capability-Gated Access

- Every `send()` invocation validates the sender's `CapabilityHandle` for the target port against the `CapabilityStore`.
- Port creation returns a `CapabilityHandle` that must be presented on subsequent operations. Handles are 128-bit unforgeable tokens generated from `RDTSC` entropy.

## 6. Scheduler

### 6.1 Task Control Block (TaskCB)

```rust
pub struct TaskCB {
	pub id:          TaskId,
	pub state:       TaskState,      // Ready | Running | Blocked | Terminated
	pub sched_class: SchedClass,     // Realtime(0-99) | Fair(100-139) | Batch | Iso
	pub cpu_ctx:     CPUContext,      // All GPRs + RIP + RFLAGS + segment regs + CR3
	pub kstack:      VirtAddr,       // Per-task kernel stack (TSS.RSP0)
	pub ustack:      VirtAddr,       // Userspace stack pointer
	pub cap_handle:  CapabilityHandle,
}
```

### 6.2 SMP-Aware Scheduling

- **Per-CPU Run Queues:** Each logical processor maintains a `RunQueue` (per-CPU, via `PER_CPU_RUN_QUEUES` array, 16 slots). Accessible via `get_per_cpu_run_queue(cpu_id)`.
- **Scheduling Classes:**
	- `Realtime(0-99)`: Static priority (FIFO ordering).
	- `Fair(100-139)`: Weighted Fair Queueing (WFQ) with virtual-runtime tracking (`wfq::pick_next()`).
	- `Batch(140)`: Low-priority background tasks.
	- `Iso(50)`: Isochronous tasks with elevated priority.
- **Priority Inheritance:** `acquire_lock_with_pi()` temporarily boosts a lock holder's priority to match the acquirer's priority (prevents priority inversion). `release_lock_with_pi()` restores the original priority. Global `LOCK_REGISTRY` tracks which task holds which lock.
- **Context Switch:** `context_switch()` (in `task/context_switch.rs`) saves/restores callee-saved GPRs (`RBP`, `RBX`, `R12-R15`), `CR3` (page table base), and `FS_BASE`/`GS_BASE` MSRs. FPU/SSE state is **not yet lazily saved** (handled by `#XF` handler, no lazy restore).

### 6.3 Process Semantics

- **Process Creation:** `SYS_SPAWN` — `process::spawn_user_process()` parses an ELF file from the VFS, allocates a fresh PML4, maps `PT_LOAD` segments, constructs a 16 KiB user stack, grants the child an empty cspace (with STDIN/STDOUT/STDERR capabilities), and enters userspace via `iretq`.
- **No `fork()` / `clone()` / `execve()`:** Each process is created fresh from an ELF file. No process sharing, CoW, or thread semantics are implemented. These are planned.
- **Process Termination:** `SYS_EXIT` — marks the task as `Zombie`, closes all FDs, notifies the parent (via `waiting_for_child` flag), and context-switches away. Parent reaps the zombie via `SYS_WAIT`.
- **No signals, no `waitpid()` semantics:** `SYS_WAIT` waits for any child to reach `Zombie` state. No signal delivery, no `WNOHANG` beyond non-blocking check.

## 7. Hardware & Driver Ecosystem

Serix discards legacy vendor-specific driver support. All hardware interaction targets **standardized interfaces** exclusively.

### 7.1 Supported Hardware Interfaces

| Interface | Bus | Driver Location | Use Case |
|---|---|---|---|
| **NVMe** | PCIe | Ring 3 server | Persistent block storage (submission/completion queue pairs) |
| **XHCI** | PCIe | Ring 3 server | USB host controller; HID input devices |
| **UEFI GOP** | Firmware | Ring 0 (boot) / Ring 3 (runtime) | Linear framebuffer; display output |
| **VirtIO-blk** | PCIe (virtio) | Ring 3 server | Virtualized block device (QEMU/KVM) |
| **VirtIO-net** | PCIe (virtio) | Ring 3 server | Virtualized network device (QEMU/KVM) |
| **LAPIC / I/O APIC** | MMIO | Ring 0 (`apic/`) | Interrupt routing; timer; IPI for SMP |
| **PS/2 Controller** | I/O port | Ring 0 (`keyboard/`) | Legacy keyboard input (fallback for non-XHCI) |

### 7.2 Driver Server Model

> **Status:** Planned. Not yet implemented.

The intended architecture is:
- Each driver server runs as a Ring 3 process with its own address space.
- PCIe BAR (Base Address Register) MMIO regions would be mapped into the driver's address space at startup.
- DMA buffers would be allocated by the kernel Memory Manager and registered in the IOMMU (when implemented) before being shared with the driver process.
- Driver–kernel communication would occur exclusively through IPC ports.

**Current reality:** The VirtIO-blk driver runs in Ring 0 (`drivers/` crate). The ext4 filesystem daemon runs in Ring 3 (`ext4d`), but it is a VFS proxy, not a hardware driver. No Ring 3 driver server framework or MMIO BAR mapping into userspace exists yet.

## 8. Zero-Copy Networking Stack

> **Status:** Planned for Phase 9. Not yet implemented.

The intended architecture:
- A Ring 3 VirtIO-net driver would map TX/RX ring buffers directly into application address space via capability grants.
- DMA buffers would be allocated from DMA-safe physical frames.
- Packet data would flow from NIC DMA → shared buffer → application without intermediate copies.
- A `#[no_std]` TCP/IP stack (e.g., `smoltcp`) would integrate as a userspace library.

**Current reality:** No networking driver or TCP/IP stack exists. Only the VirtIO-blk block driver is implemented.

## 9. Security Model — POSIX-to-Capability Bridge

### 9.1 Capability Store

- All kernel resources (memory regions, IPC ports, file descriptors, interrupt vectors, I/O devices) are represented as `Capability` objects in a global `CapabilityStore` (`BTreeMap<CapabilityHandle, Capability>`).
- `CapabilityHandle` is a 128-bit unforgeable token. Handles are generated from hardware entropy (`RDTSC` + `RDRAND` mixing).
- Operations: `grant(task_id, capability)` → `CapabilityHandle`; `revoke(handle)`; `validate(handle, expected_type)` → `bool`.

### 9.2 Access Control

**Current state:** Access control is capability-based. Every resource (inode, IPC port, device) is represented as a `CapabilityRecord` in the global `CapabilityStore`. Operations require a valid `CapabilityRef { slot, generation }` on the syscall path. The kernel validates:
1. Slot belongs to the caller's cspace
2. Generation matches current slot generation (stale rejection)
3. Record is not revoked and not expired
4. Required rights (READ, WRITE, METADATA, etc.) are present

**Planned:** A POSIX DAC bridge daemon would translate legacy UID/GID/mode semantics into capability tickets. This is not yet implemented.

## 10. Initialization — Boot Task and Kernel Shell

There is **no Server Manager (PID 1)** in the current implementation. The kernel initializes all subsystems in `_start()` and spawns a fixed set of tasks:

### 10.1 Actual Bootstrap Sequence

1. **Kernel entry** (`_start()`): Serial init → disable PIC/enable APIC → register interrupt handlers → load IDT → enable interrupts
2. **Executor init:** Global `EXECUTOR` initialized
3. **Memory init:** Frame allocator, heap, page table mapper from Limine responses
4. **Audit ring:** `capability::init_audit()` — initializes the ~6KB audit buffer
5. **Graphics:** Framebuffer console with blue background
6. **Drivers:** VirtIO block device enumeration via PCI
7. **Filesystem:** FAT32/ext2/ext4 registration, VFS mount table (`/` and `/dev/`), page cache, ext4d ELF inserted into VFS root
8. **LES policy:** Default deny rules loaded (`/etc/shadow`, `/proc/serix/cap-audit`)
9. **Scheduler:** Per-CPU run queues initialized; core type detected
10. **SMP:** LAPIC timer started; AP bootstrap callbacks registered; `AP_READY_PTR` signaled
11. **User tasks:** ext4d daemon spawned (ext4 filesystem), policyd daemon spawned (LES policy engine with TOML config), kshell (kernel-resident Ring-0 shell)
12. **Idle loop:** `hlt()` in a loop; preempted by LAPIC timer interrupts (~625 Hz)

## 11. Subsystem Documentation Index

| Document | Description |
|---|---|
| [BOOT_PROCESS.md](BOOT_PROCESS.md) | Limine handoff, GDT/IDT/APIC init, kernel entry |
| [MEMORY_LAYOUT.md](MEMORY_LAYOUT.md) | Complete virtual/physical memory map |
| [INTERRUPT_HANDLING.md](INTERRUPT_HANDLING.md) | IDT vectors, APIC programming, handler registration |
| [HAL_API.md](HAL_API.md) | Serial, CPUID, topology detection |
| [GRAPHICS_API.md](GRAPHICS_API.md) | Framebuffer console, drawing primitives |
| [KERNEL_API.md](KERNEL_API.md) | Syscall interface, userspace ABI |
| [ROADMAP.md](ROADMAP.md) | Development phases and milestones |

## 12. Crate Map

```
serix/
├── kernel/        # Entry point, syscall dispatch, GDT, process spawning, kshell, LES bridge, procfs
├── memory/        # PML4 management, heap, frame allocator, kstack allocator, VMA manager
├── hal/           # Serial I/O, CPUID, CPU topology, MSR access, I/O ports
├── apic/          # LAPIC, I/O APIC, LAPIC timer, SMP
├── idt/           # IDT construction, exception/IRQ handler stubs
├── graphics/      # Framebuffer console, text rendering, drawing primitives
├── task/          # TaskCB, scheduler, context_switch(), async executor, WFQ
├── capability/    # CapabilityStore, CapabilityHandle, cspace, audit ring, validation
├── ipc/           # Port-based IPC, Message, IpcSpace, notification ports
├── vfs/           # INode trait, RamFile, RamDir, mount table (longest-prefix), page cache
├── fs/            # FAT32 + ext2 drivers, ext4 parser/kernel stubs, block cache, policy engine
├── loader/        # ELF64 parser, PT_LOAD segments
├── drivers/       # PCI enumeration, VirtIO-blk, ConsoleDevice
├── keyboard/      # PS/2 scancode translation, key event queue
├── ulib/          # Userspace syscall wrappers, capability refs, fmt, heap
├── ext4d/         # Ring-3 ext4 filesystem daemon (embedded in kernel binary)
├── policyd/       # Ring-3 policy service daemon (embedded in kernel binary, spawned at boot, TOML config, hot-reload)
└── util/          # Panic handler, shared utilities
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines and [ROADMAP.md](ROADMAP.md) for current priorities.
