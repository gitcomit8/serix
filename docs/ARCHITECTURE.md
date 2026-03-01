# Serix Kernel Architecture

> **Target ISA:** x86_64 (AMD64) · **Privilege Model:** Ring 0 / Ring 3 Hybrid · **Current Release:** v0.0.5

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

Serix is a **hybrid-kernel** operating system written in `#![no_std]` Rust for the x86_64 architecture. The kernel enforces a **capability-based security model** and provides a **Linux Executable Subsystem (LES)** ABI translation layer capable of running unmodified Linux ELF binaries without recompilation.

- The architecture pins latency-critical subsystems (SMP scheduler, VFS, Memory Manager, IPC Router) in **Ring 0** for deterministic dispatch.
- Crash-prone and high-latency subsystems (device drivers, filesystem daemons, network stack) are isolated as **Ring 3 userspace servers** communicating via synchronous/asynchronous IPC.
- A **POSIX-to-Capability Authorization Bridge** translates legacy DAC (UID/GID/mode bits) semantics intercepted from the Linux ABI into fine-grained cryptographic capability tickets, preserving the security invariant that no resource access bypasses the `CapabilityStore`.

This is **not** a drop-in Linux replacement. Serix targets **gradual workload migration**: applications linked against glibc or musl execute atop the LES translation layer while native Serix binaries call kernel services directly through capability-gated IPC.

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

### 2.2 Ring 3 — Userspace Server Processes

These subsystems run as isolated processes in their own address spaces. Faults in any server trigger process-level restart without kernel panic.

| Server | Transport | Description |
|---|---|---|
| **Ext4 Filesystem Daemon** | Synchronous IPC | Parses extent trees, manages journal, serves VFS `read`/`write` dispatches |
| **Network Stack** | Zero-copy shared buffers | TCP/IP processing on VirtIO-net ring buffers mapped into application address space |
| **Block Driver (VirtIO-blk)** | Asynchronous IPC | Submits I/O descriptors to VirtIO virtqueues; interrupt-driven completion |
| **USB/Input Driver (XHCI)** | Asynchronous IPC | Manages XHCI host controller rings; delivers HID events via IPC ports |
| **Display Server (GOP)** | Shared framebuffer | Composites client surfaces onto UEFI GOP linear framebuffer |
| **POSIX-to-Capability Bridge** | Synchronous IPC | Translates `UID`/`GID`/`mode` DAC checks into `CapabilityHandle` tickets |
| **Server Manager (PID 1)** | — | Bootstrap orchestrator; constructs dependency DAG; see [§10](#10-initialization--server-manager-pid-1) |

### 2.3 Ring Transition ABI

- **User → Kernel:** `SYSCALL` instruction. RCX/R11 saved by hardware. Kernel swaps to per-task kernel stack via `TSS.RSP0`. SFMASK clears `IF` during entry.
- **Kernel → User:** `SYSRET` restores RCX → RIP, R11 → RFLAGS. For full context restores (e.g., signal delivery), `IRETQ` is used instead.
- **IPC Fastpath:** For small messages (≤128 bytes), the IPC Router performs a direct register-mediated transfer between sender and receiver `TaskCB` kernel stacks without copying through intermediate buffers.

## 3. Linux Executable Subsystem (LES) — ABI Translation Layer

The LES enables execution of unmodified Linux ELF binaries by translating Linux `syscall` invocations into native Serix IPC dispatches.

### 3.1 Syscall Entry Trampoline

- The `SYSCALL` entry point (`kernel/src/syscall.rs`) is a **zero-overhead naked function** (`#[naked]`) emitting raw assembly via `naked_asm!`.
- Register state is marshaled according to the Linux x86_64 ABI: `RAX`=syscall number, `RDI`/`RSI`/`RDX`/`R10`/`R8`/`R9`=arguments.
- Serix intentionally shares Linux syscall numbers (e.g., `SYS_WRITE=1`, `SYS_READ=0`, `SYS_EXIT=60`) so the trampoline functions primarily as an **IPC routing layer** — it decodes the syscall number and dispatches to the appropriate Ring 0 handler or Ring 3 server via IPC.

### 3.2 Translation Pipeline

```
	Linux ELF Binary
	    │
	    ▼
	SYSCALL instruction (Ring 3 → Ring 0)
	    │
	    ▼
	┌──────────────────────────────────┐
	│  Naked ASM Trampoline            │
	│  • Save RCX/R11/callee-saved     │
	│  • Swap to kernel stack (TSS)    │
	│  • Validate userspace pointers   │
	└──────────┬───────────────────────┘
	           │
	           ▼
	┌──────────────────────────────────┐
	│  #[no_std] Rust Dispatch Layer   │
	│  • Match RAX → syscall handler   │
	│  • unsafe struct translations    │
	│  • Capability ticket injection   │
	└──────────┬───────────────────────┘
	           │
	     ┌─────┴──────┐
	     ▼            ▼
	Ring 0 Handler   IPC to Ring 3 Server
	(fast path)      (e.g., Ext4 daemon)
```

### 3.3 Struct Translation

- Linux kernel structs (`struct stat`, `struct iovec`, `struct sockaddr`) are re-declared as `#[repr(C)]` Rust types within the LES crate.
- Translation occurs in tightly optimized `unsafe` blocks that perform zero-copy reinterpretation via pointer casts where alignment permits, falling back to field-by-field copy for misaligned or variable-length structures.
- The translation layer injects a `CapabilityHandle` into each request before forwarding to internal subsystems, bridging the Linux file-descriptor model to Serix's capability-gated resource access.

### 3.4 ELF Loading & Process Bootstrapping

- The ELF loader (`loader/`) parses `PT_LOAD`, `PT_INTERP`, and `PT_GNU_STACK` program headers.
- For dynamically linked binaries, `PT_INTERP` names the runtime linker (e.g., `/lib64/ld-linux-x86-64.so.2`), which is loaded from the Serix VFS.
- An **auxiliary vector (auxv)** is constructed on the user stack conforming to the Linux ABI (`AT_PHDR`, `AT_PHENT`, `AT_PHNUM`, `AT_ENTRY`, `AT_BASE`, `AT_PAGESZ`, `AT_RANDOM`).
- `execve()` semantics: parse ELF headers, allocate a fresh PML4 via `create_user_page_table()`, map `PT_LOAD` segments with correct `R/W/X` page flags, construct `auxv`/`envp`/`argv` on the user stack, and enter userspace via `IRETQ` to the ELF entry point (or `PT_INTERP` entry for dynamic binaries).

## 4. Memory Management Subsystem

### 4.1 Address Space Layout

| Region | Virtual Address Range | Description |
|---|---|---|
| **Userspace** | `0x0000_0000_0000_0000` – `0x0000_7FFF_FFFF_FFFF` | Per-process; isolated via CR3 swap |
| **Non-canonical hole** | `0x0000_8000_0000_0000` – `0xFFFF_7FFF_FFFF_FFFF` | Hardware-enforced gap (`#GP` on access) |
| **HHDM (Physical RAM)** | `0xFFFF_8000_0000_0000` + offset | Direct map of all physical memory; offset from Limine `HhdmRequest` |
| **Kernel Heap** | `0xFFFF_8000_4444_0000` – `0xFFFF_8000_4454_0000` | 1 MiB; `linked_list_allocator`; growable via frame allocator |
| **Kernel Code/Data** | Upper canonical half (Limine-assigned) | Higher-half kernel; PML4 entries 256–511 shared across all address spaces |

### 4.2 Demand Paging & Unified Page Cache

- **Page Fault (`#PF`) Handling:** The IDT vector 14 handler traps into the Ring 0 Memory Manager, which inspects the faulting VMA (Virtual Memory Area) descriptor.
- **File-Backed Pages:** If the VMA is backed by an `INode`, the Memory Manager dispatches a **synchronous IPC** to the VFS to issue a DMA block fetch from the backing store (Ext4 daemon → VirtIO-blk driver).
- **Unified Page Cache:** Fetched pages are inserted into a **concurrent radix tree** (lockless read path, fine-grained write locks) indexed by `(INode, page_offset)`. Subsequent faults and `read()` calls for the same block resolve from cache without I/O.
- **Anonymous Pages:** `mmap(MAP_ANONYMOUS)` faults allocate zeroed frames on demand from the frame allocator.
- **Copy-on-Write (CoW):** `fork()`/`clone(CLONE_VM=0)` marks all writable pages as read-only in both parent and child PML4s. A subsequent write fault triggers CoW duplication.

### 4.3 Frame Allocator

- **Boot Phase:** `StaticBootFrameAllocator` pre-populates a frame list from the Limine memory map (`MemoryMapEntryType::USABLE` regions).
- **Runtime Phase:** A buddy allocator manages free frames in O(log n) allocation/deallocation with per-order freelists. NUMA-aware allocation is deferred until SMP topology enumeration is complete.

### 4.4 IOMMU Integration

- **Intel VT-d / AMD-Vi** page tables are programmed to restrict DMA-capable devices to their assigned physical frame regions.
- IOMMU remapping tables are parsed from the ACPI **DMAR** (DMA Remapping Reporting) table.
- VirtIO-blk and VirtIO-net ring buffer physical addresses are registered in IOMMU page tables before the corresponding Ring 3 driver servers are started, ensuring DMA isolation.

## 5. IPC Router

### 5.1 Message Passing Model

- **Ports:** Each IPC endpoint is a `Port` — a `Mutex<VecDeque<Message>>` with a configurable depth (default 32).
- **Messages:** Fixed 128-byte payload (`Message { sender_id, id, len, data: [u8; 128] }`).
- **IPC Space:** A global `BTreeMap<PortId, Port>` registry (`IPC_GLOBAL`) for port lookup.
- **Operations:** `send(port_id, msg)` enqueues; `receive(port_id)` dequeues (currently non-blocking; blocking semantics planned via scheduler integration).

### 5.2 IPC Fastpath

- For latency-critical paths (e.g., `#PF` → VFS page fetch), the IPC Router performs a **direct context switch** from sender to receiver, transferring the message in registers (`RDI`–`R9`) without kernel buffer copies.
- This fastpath bypasses the `VecDeque` entirely and is used when the receiver is blocked in `receive()` at the moment of `send()`.

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

- **Per-CPU Run Queues:** Each logical processor maintains a local `VecDeque<TaskId>` run queue, reducing cross-core lock contention.
- **Scheduling Classes:**
	- `Realtime(0-99)`: FIFO with static priority. Preempts all lower classes.
	- `Fair(100-139)`: Weighted Fair Queueing (WFQ) with dynamic nice-adjusted virtual runtime.
	- `Batch`: Background; scheduled only when no `Realtime` or `Fair` tasks are runnable.
	- `Iso`: Isolated; pinned to specific cores (for real-time workloads requiring deterministic jitter).
- **Load Balancing:** A periodic timer callback (LAPIC timer) examines per-CPU queue depths and migrates tasks between cores. Migration decisions factor in **cache warmth tracking** (last-run-core affinity) to minimize L1d/L2 thrashing.
- **Context Switch:** Assembly routine `context_switch()` saves/restores all callee-saved registers, `CR3`, `FS_BASE`/`GS_BASE` MSRs, and FPU/SSE/AVX state via `XSAVE`/`XRSTOR`.

### 6.3 Process Semantics & Linux `clone()` Mapping

- Linux `clone()` flags map to native `TaskCB` construction:
	- `CLONE_VM`: Share parent's PML4 (threads). Without: CoW-duplicate PML4 (fork).
	- `CLONE_FS`: Share `cwd` and `umask`. Without: Copy.
	- `CLONE_FILES`: Share file descriptor table. Without: Duplicate.
	- `CLONE_SIGHAND`: Share signal handler table.
- `execve()`: Parse ELF `PT_INTERP`, construct `auxv`, replace address space, reset signal dispositions to `SIG_DFL`, and jump to ELF entry.
- **Userspace Interrupt Delivery:** Hardware interrupts destined for userspace (e.g., `SIGSEGV` on `#PF` in user VMA) are routed through **event channels** — IPC ports bound to signal numbers — enabling the target task to handle faults asynchronously without kernel-mediated signal frames.

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

- Each driver server runs as a Ring 3 process with its own address space.
- PCIe BAR (Base Address Register) MMIO regions are mapped into the driver's address space by the Server Manager at startup.
- DMA buffers are allocated by the kernel Memory Manager and registered in the IOMMU before being shared with the driver process.
- Driver–kernel communication occurs exclusively through IPC ports; the driver cannot directly access kernel data structures.

## 8. Zero-Copy Networking Stack

### 8.1 Architecture

The network stack employs a **kernel-bypass architecture** where the Ring 3 network driver maps VirtIO-net TX/RX ring buffers directly into the target application's address space.

```
	┌─────────────┐       ┌──────────────────┐       ┌────────────────┐
	│ Application  │◄─────►│ Network Server   │◄─────►│ VirtIO-net HW  │
	│ (Ring 3)     │ mmap  │ (Ring 3)         │ MMIO  │ (virtqueues)   │
	│              │ shared│                  │       │                │
	│  ┌────────┐  │ buf   │  ┌────────────┐  │       │                │
	│  │ TCP/IP │  │       │  │ Ring Mgmt  │  │       │                │
	│  │ Stack  │  │       │  │ + DMA      │  │       │                │
	│  └────────┘  │       │  └────────────┘  │       │                │
	└─────────────┘       └──────────────────┘       └────────────────┘
```

### 8.2 Buffer Management

- The VirtIO-net driver allocates TX/RX ring buffers from DMA-safe physical frames registered in the IOMMU.
- These buffers are shared into the application's address space via the **Capability Store**: the driver grants a `CapabilityHandle` of type `MemoryRegion` to the application, which maps the buffer into its VMA.
- Packet data flows from NIC DMA → shared buffer → application without any intermediate copies.
- The application's TCP/IP stack (running in-process as a library, e.g., `smoltcp`) processes packets directly from the shared buffer.

## 9. Security Model — POSIX-to-Capability Bridge

### 9.1 Capability Store

- All kernel resources (memory regions, IPC ports, file descriptors, interrupt vectors, I/O devices) are represented as `Capability` objects in a global `CapabilityStore` (`BTreeMap<CapabilityHandle, Capability>`).
- `CapabilityHandle` is a 128-bit unforgeable token. Handles are generated from hardware entropy (`RDTSC` + `RDRAND` mixing).
- Operations: `grant(task_id, capability)` → `CapabilityHandle`; `revoke(handle)`; `validate(handle, expected_type)` → `bool`.

### 9.2 POSIX-to-Capability Authorization Bridge

- A Ring 3 daemon intercepts POSIX permission checks originating from the LES layer (e.g., `open()` with `O_RDWR` on a file owned by `uid=1000`, `mode=0644`).
- The bridge daemon translates these DAC semantics into dynamic capability tickets:
	- Consult a policy database mapping `(UID, GID, path, mode)` → `CapabilityType` + permission set.
	- Mint a time-bounded `CapabilityHandle` with the resolved permissions.
	- Return the handle to the LES layer, which attaches it to subsequent VFS/IPC requests.
- This design ensures that **no Linux binary can escalate beyond the capabilities granted by the bridge**, even if it exploits a vulnerability in the LES translation layer.

## 10. Initialization — Server Manager (PID 1)

The Server Manager is the first userspace process spawned by the kernel. It orchestrates the boot of all Ring 3 servers.

### 10.1 Bootstrap Sequence

1. **Kernel handoff:** The kernel loads the Server Manager ELF from the ramdisk VFS, creates its address space, and enters Ring 3 via `IRETQ`.
2. **IPC Router memory:** The Server Manager maps the IPC Router's shared port registry into its address space (pre-allocated by the kernel).
3. **tmpfs VFS root:** Mounts a `tmpfs` as the root filesystem (`/`), populating `/dev`, `/proc`, `/sys` mount points.
4. **LES translation tables:** Initializes the Linux syscall number → Serix IPC port mapping table.
5. **Dependency DAG construction:** Parses a declarative service manifest (e.g., `/etc/serix/services.toml`) to determine server start order.
6. **Driver server binding:** For each driver server in the DAG:
	- Enumerates PCIe configuration space for matching `(vendor_id, device_id)` tuples.
	- Maps the device's BAR MMIO regions into the driver server's address space.
	- Registers DMA buffer frames in the IOMMU.
	- Spawns the driver server process and grants it the appropriate `CapabilityHandle` set.
7. **Filesystem servers:** Starts the Ext4 daemon, which reads the superblock at offset `0x400` from the VirtIO-blk device and mounts the partition.
8. **User session:** Spawns `serix-sh` (the default shell) or the configured login manager.

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
├── kernel/        # Entry point, syscall dispatch, GDT, Server Manager bootstrap
├── memory/        # PML4 management, heap, frame allocator, demand paging
├── hal/           # Serial I/O, CPUID, CPU topology, MSR access
├── apic/          # LAPIC, I/O APIC, LAPIC timer, IPI broadcast
├── idt/           # IDT construction, exception/IRQ handler stubs
├── graphics/      # Framebuffer console, text rendering, drawing primitives
├── task/          # TaskCB, scheduler, context_switch(), async executor
├── capability/    # CapabilityStore, CapabilityHandle, grant/revoke/validate
├── ipc/           # Port-based IPC, Message, IpcSpace, fastpath
├── vfs/           # INode trait, RamFile, RamDir, mount table, page cache
├── loader/        # ELF64 parser, PT_LOAD/PT_INTERP, auxv construction
├── drivers/       # PCI enumeration, VirtIO-blk, ConsoleDevice
├── keyboard/      # PS/2 scancode translation, key event queue
└── ulib/          # Userspace syscall wrappers, init binary
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines and [ROADMAP.md](ROADMAP.md) for current priorities.
