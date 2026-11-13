# Serix: General-Purpose OS Vision, Modern DOS Feature Study, and v0.1.0 Release Plan

> **Document type:** Architecture direction, feature inventory, and release proposal  
> **Target:** Serix x86-64 / AMD64, `#![no_std]` Rust, Limine boot flow  
> **Current release baseline supplied by project metadata:** `v0.0.8`  
> **Repository snapshot reviewed:** `dev` branch, commit `2550e02567d9db99413c52951604b01e791f7de7` (13 November 2026)  
> **Strategic objective:** Build a credible general-purpose operating system and long-term Linux alternative
> **Status:** Proposal — not an assertion that the features below are implemented

## Contents

- [0. Product objective and strategic direction](#0-product-objective-and-strategic-direction)
- [1. Status legend and interpretation rules](#1-status-legend-and-interpretation-rules)
- [Part I — Counterfactual modern DOS feature model](#part-i--counterfactual-modern-dos-feature-model-reference-study-only)
  - [2. Boot, firmware, and system initialization](#2-boot-firmware-and-system-initialization)
  - [3. CPU, memory, and execution model](#3-cpu-memory-and-execution-model)
  - [4. Process and application model](#4-process-and-application-model)
  - [5. Filesystem, storage, and volume management](#5-filesystem-storage-and-volume-management)
  - [6. Device model, drivers, and hardware discovery](#6-device-model-drivers-and-hardware-discovery)
  - [7. System-call ABI and I/O model](#7-system-call-abi-and-io-model)
  - [8. Shell, command language, and userland utilities](#8-shell-command-language-and-userland-utilities)
  - [9. Startup services, system policy, and security](#9-startup-services-system-policy-and-security)
  - [10. Networking and remote resources](#10-networking-and-remote-resources)
  - [11. Time, locale, power, and platform services](#11-time-locale-power-and-platform-services)
  - [12. Diagnostics, recovery, and system maintenance](#12-diagnostics-recovery-and-system-maintenance)
  - [13. Graphics, applications, compatibility, and developer tools](#13-graphics-applications-compatibility-and-developer-tools)
- [Part II — What this means for Serix](#part-ii--what-this-means-for-serix)
  - [14. DOS concepts translated into a general-purpose Serix architecture](#14-dos-concepts-translated-into-a-general-purpose-serix-architecture)
  - [15. What Serix must not inherit from DOS](#15-what-serix-must-not-inherit-from-dos--and-what-it-must-not-oversimplify)
  - [16. Current Serix snapshot](#16-current-serix-snapshot-strengths-and-unfinished-contracts)
  - [16A. Credible Linux alternative scorecard](#16a-what-credible-linux-alternative-means)
- [Part III — Recommended v0.1.0 definition](#part-iii--recommended-v010-definition)
  - [17. What v0.1.0 should mean](#17-what-v010-should-mean)
  - [18. v0.1.0 release gates](#18-v010-release-gates--split-priority-matrix)
  - [19. End-to-end demonstration](#19-the-v010-end-to-end-demonstration)
  - [20. Implementation sequence](#20-recommended-implementation-sequence)
  - [21. Long-range roadmap](#21-long-range-roadmap-toward-a-credible-general-purpose-os)
  - [22. Decisions to make now](#22-decisions-to-make-now)
  - [23. What not to do before v0.1.0](#23-what-not-to-do-before-v010)
  - [24. Definition of done](#24-definition-of-done-for-v010)
- [Appendix A — Historical DOS concepts](#appendix-a--historical-ms-dos-concepts-and-modern-equivalents)
- [Appendix B — Repository and technical references](#appendix-b--repository-and-technical-references)
- [Final recommendation](#final-recommendation)
---

## 0. Product objective and strategic direction

Serix is intended to become a **credible, general-purpose operating system and, over the long term, a realistic alternative to Linux**. It is not a hobby kernel, a retrocomputing exercise, a DOS clone, or a permanently QEMU-only research artifact.

That ambition changes how every feature in this document should be interpreted. MS-DOS is the historical reference being studied here because it had a clear, cohesive operating environment: boot, shell, executable loading, files, devices, drivers, configuration, diagnostics and utilities. The purpose is to extract useful product and interface ideas, then redesign them around modern hardware, memory protection, multicore execution, security, networking and application requirements.

**DOS is an architectural and product-study reference. Linux is the long-term competitive benchmark. Serix must have its own identity and architecture.**

### Product objective

Build a modern x86-64 general-purpose OS with:

1. **A reliable kernel foundation:** memory management, scheduling, process isolation, IPC, interrupts, resource ownership and a documented native ABI.
2. **A complete operating environment:** init/service supervision, shell, core utilities, system configuration, logging, recovery and package/update workflows.
3. **Modern hardware support:** firmware discovery, PCIe, storage, USB, input, display, audio, power management and eventually a broad driver ecosystem.
4. **A complete storage and networking stack:** persistent filesystems, data integrity, permissions, network protocols, sockets and remote resources.
5. **A usable application platform:** native binaries, libraries, dynamic linking, SDK/toolchain support, package management and compatibility options where there is real demand.
6. **Desktop and headless/server capability:** Serix must not be trapped as a command-line-only system. A shell is essential infrastructure; graphics and application environments are long-term product requirements.
7. **Security and maintainability:** capability-based authority, sane defaults, isolation, updates, recovery and operational observability.
8. **Evidence-driven releases:** every claim backed by reproducible tests, a declared hardware matrix and clear known limitations.

### Strategic positioning

Serix should be designed as a **general-purpose hybrid operating system**. The initial development and validation path can be narrow; the architecture and roadmap must not be.

| Dimension | Near-term engineering focus | Long-term product requirement |
|---|---|---|
| Platform | x86-64, QEMU/OVMF, reproducible VirtIO devices | A documented and expanding real-hardware compatibility matrix |
| User experience | Ring 3 shell and system utilities | Both desktop and headless/server workflows |
| Applications | Native static ELF64 programs | Dynamic applications, libraries, SDK, packages and compatibility runtimes |
| Storage | One reliable persistent filesystem path | Multiple robust filesystems, partitioning, encryption options, recovery and backups |
| Networking | No network claim until a tested stack exists | Modern IPv4/IPv6, TCP/UDP, DNS, DHCP, sockets, firewall and network services |
| Drivers | Existing kernel drivers while boundaries are hardened | Resource-managed drivers/services with isolation where practical and justified |
| Compatibility | Stable Serix-native ABI | Deliberate POSIX/API compatibility and possibly selected Linux binary compatibility |
| Security | Close current capability and policy gaps | Multi-user access controls, sandboxing, secure boot/update path and auditable policy |
| Ecosystem | Buildable native user programs | Package repository, application ports, developer tools and community contribution process |

### What “Linux alternative” does and does not mean

It does **not** mean reproducing Linux internals, copying every Linux syscall, or declaring feature parity. It means that Serix should eventually be capable of serving real users for clearly defined general-purpose workloads without requiring Linux as the host operating system.

That requires more than a kernel that boots. It requires hardware support, an application platform, installers, updates, documentation, security response, diagnostic tools, software availability and sustained maintenance. A technically elegant microkernel that cannot run useful applications or support common hardware is not a credible alternative.

Linux compatibility is a possible adoption strategy, not a v0.1 requirement and not a substitute for a native Serix ABI. The project should first define a stable native execution environment, then make an evidence-based decision about POSIX coverage, libc targets, compatibility shims and the cost/benefit of Linux binary compatibility.

### The direction in one sentence

**Build Serix as a secure, reliable, hardware-capable general-purpose x86-64 operating system, using a hybrid kernel and replaceable services, with DOS-like operational clarity and Linux-class breadth as the long-term benchmark.**

## 1. Status legend and interpretation rules

The Serix status columns below use the repository documentation reviewed on the `dev` branch. They are not independent test results from this review.

| Marker | Meaning |
|---|---|
| **Implemented / tested** | The repository describes a runnable path and documented QEMU validation. This is not a production-readiness claim. |
| **Partial** | Some implementation exists, but coverage, integration, isolation, recovery, or acceptance tests remain incomplete. |
| **Planned** | The roadmap describes the feature, but there is no complete runnable path. |
| **Not a v0.1 goal** | Intentionally deferred; not a release blocker. |
| **Proposed** | A recommendation in this document, not current repository functionality. |

Important distinctions:

- A subsystem existing in source code does not mean its full contract is stable.
- A userspace filesystem daemon does not mean hardware drivers already run in userspace.
- PCI enumeration does not equal a PCI resource manager, hotplug implementation, or device-driver framework.
- Parsing ACPI tables does not equal complete power management or IOMMU support.
- Allocating a page cache does not mean filesystem reads and writes use it.
- Having a kernel shell does not equal having an independent userspace shell or a usable external-program environment.

---

# Part I — Counterfactual modern DOS feature model (reference study only)

## 2. Boot, firmware, and system initialization

A modern DOS-like system would keep DOS's understandable startup path but use current firmware and platform discovery mechanisms.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today (repository snapshot) | Serix direction |
|---|---|---|---|
| Firmware boot | UEFI boot; retain legacy boot only if explicitly supported | Limine BIOS + UEFI ISO path documented | **Keep.** Treat Limine as the boot contract; validate both paths in QEMU. |
| Boot protocol | Versioned boot handoff containing memory map, framebuffer, CPU, ACPI and boot-volume information | Limine request/response protocol, memory map, HHDM and framebuffer | **Harden.** Validate every response and record boot configuration in diagnostics. |
| Partition discovery | GPT first; MBR only for compatibility | Block device and filesystem probing described; full GPT partition manager is not established by reviewed docs | **Add after v0.1** unless boot-volume discovery blocks basic disk use. |
| System image | Kernel, initramfs/system image, configuration, service manifests | Kernel plus embedded `ext4d` and `policyd`; `make init` is documented in developer instructions | **Move toward explicit boot image composition.** Reduce reliance on embedded daemons as services mature. |
| Startup configuration | Declarative TOML or similarly simple format with validation | `/etc/serix/policy.toml` is described; no general service manifest is established | **Add.** Separate boot-critical config, service configuration, and security policy. |
| Init/service manager | PID 1 starts services in dependency order, monitors exits, restarts eligible services | `ext4d` and `policyd` are spawned; a general service manager/restart contract is not documented | **P0 for v0.1.** Establish an explicit Ring 3 init process and service lifecycle. |
| Recovery boot | Minimal recovery shell, read-only mount option, diagnostic mode | Kernel-resident `kshell` provides a useful diagnostic foundation | **Preserve `kshell` as recovery/debug shell**, not as the normal user interface. |
| Boot diagnostics | Structured boot log, phase timings, device inventory, failure reason | Serial and framebuffer logs; boot checkpoints documented | **Improve.** Stable log levels, subsystem tags, and a boot summary. |
| Secure boot | Signed boot components and policy-controlled trust chain | Not a documented current feature | Defer beyond v0.1; design the boot chain so signature verification can be added. |

### Recommended startup sequence

```text
UEFI firmware
    ↓
Limine bootloader
    ↓
Serix kernel: CPU / memory / interrupt / SMP initialization
    ↓
Kernel creates initial address space and core IPC objects
    ↓
Kernel starts Ring 3 init (PID 1)
    ↓
init mounts required filesystems and starts core services
    ↓
init starts the interactive Ring 3 shell (rsh)
    ↓
User invokes external commands and applications
```

The kernel should still provide a minimal emergency path when userspace init fails. That does not justify keeping the normal shell in Ring 0.

---

## 3. CPU, memory, and execution model

A modern DOS designed for current hardware would retain the simplicity of a small operating environment while using the CPU's full protection and execution capabilities.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| CPU architecture | 64-bit x86-64 baseline; architecture-specific HAL | x86-64 / AMD64 | Keep x86-64 as the supported target until the ABI and hardware layers mature. Do not dilute effort with a second architecture yet. |
| Privilege | User/kernel privilege separation, supervisor protections, NX and W^X | Ring 0 kernel and separate Ring 3 processes | Make all privilege boundaries explicit and testable. |
| Address spaces | Isolated process address spaces; controlled shared mappings | Per-process PML4 / CR3 switching described | Keep; add stress tests for switching and teardown. |
| SMP | Multi-core and multi-thread scheduling | AP startup, per-CPU run queues, WFQ and priority inheritance described; max configured slots 16 | Stabilize correctness before optimizing. Work stealing and broad CPU topology policy can wait. |
| Hybrid CPUs | Core-type detection, capacity-aware scheduling and affinity controls | CPUID-based P-core/E-core detection infrastructure described; advanced policy not complete | Keep detection; defer sophisticated placement until basic scheduler correctness is demonstrated. |
| Scheduling | Preemptive fair scheduling, priority classes, blocking and wakeups | LAPIC timer-driven preemption, per-CPU WFQ and priority inheritance | Stress-test context switches, wakeups, starvation, PI, and cross-CPU behavior. |
| Threads | Native threads with shared address space and TLS | `clone()` / thread semantics are planned, not complete | Not needed for v0.1 if process-based commands work. Add after process lifecycle is stable. |
| Process lifecycle | Spawn, exit, wait, environment, arguments, process metadata | ELF spawn, exit, wait and PID/PPID calls are described; no fork/execve/signals | Prefer a clean native `spawn(path, argv, envp)` contract. Do not require Linux `fork()` semantics for v0.1. |
| SIMD/FPU state | Correct x87/SSE/AVX state management across task switches | Architecture documentation flags FPU/SSE state handling as incomplete | **Release blocker.** A general userspace environment cannot safely ignore extended CPU state. |
| Timers | Monotonic clock, wall clock, high-resolution timers, per-CPU clockevents | LAPIC timer is used; PIT/HPET not used as runtime timer | Stabilize a monotonic clock API; add RTC/wall-clock source and sleep syscall. PIT/HPET support is not automatically a v0.1 blocker. |
| Memory allocation | Physical frame allocation/free, kernel heap, user heap, VMAs, demand paging | Frame allocator/VMA/CoW/page-cache work exists; architecture doc notes no frame deallocation | **Release blocker to resolve:** frame reclamation, address-space teardown, stack limits and leak tests. |
| Virtual memory | `mmap`, anonymous mappings, private/shared maps, COW, guard pages, page cache | File-backed mmap/demand paging/CoW described; page cache not integrated into file I/O | Validate page-fault correctness and resource reclamation. Defer swap and huge pages. |
| DMA | Explicit DMA buffer ownership and mapping, IOMMU domains where hardware supports them | VirtIO DMA uses HHDM physical addresses; IOMMU discovery exists but isolation is not implemented | Do not delegate arbitrary device DMA to Ring 3 until buffer ownership and IOMMU policy are real. |
| NUMA | Topology-aware physical allocation and CPU scheduling | Not established in current implementation | Defer. |

### Modern memory feature set

A complete modernized DOS concept could eventually include:

- Per-process virtual address spaces.
- Demand paging and copy-on-write.
- Anonymous and file-backed mappings.
- Guard pages for kernel and user stacks.
- Kernel heap and userspace heap APIs.
- Physical frame allocation and reclamation.
- Shared memory for explicitly authorized processes.
- Memory-mapped device regions only through explicit resource grants.
- Page cache with dirty tracking and unified writeback.
- Huge pages where useful and supported.
- Optional swap/pagefile support.
- Memory accounting per process and system-wide.
- NUMA-aware allocation on suitable hardware.

**Serix priority:** complete correct allocation, mapping, reclamation and isolation before adding swap, huge pages, or advanced NUMA optimizations.

---

## 4. Process and application model

The modern equivalent of DOS's `.COM`, `.EXE`, batch files, PSP, and `EXEC` should be a small and documented application ABI — not a literal reproduction of those formats.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Executable format | ELF64 static executables first; dynamic ELF later | ELF64 `PT_LOAD` support; static binaries are the expected path | Keep ELF64 as the native format. Add loader validation and documented ABI. |
| Process creation | Create a protected process from an executable path, argv and environment | `SYS_SPAWN` loads an ELF path into a fresh process | Build on `SYS_SPAWN`; make argument/environment passing a documented contract. |
| Process replacement | Optional `exec`-style image replacement | `execve()` is not implemented | Defer for v0.1. A new process can be spawned per command. |
| Parent/child | Parent can wait and retrieve exit status | `SYS_WAIT` and `SYS_EXIT` described; wait semantics are limited | Define exit status, child ownership, zombie cleanup, and wait errors. |
| Threads | Optional native threads | `clone()` / TLS planned | Defer until process semantics and FPU state are correct. |
| Environment | Per-process environment block, inherited explicitly | Process environment ABI not fully documented | Add argv/envp to spawn before shell scripting depends on it. |
| File descriptors | Per-process descriptor table, dup, close-on-exec and inheritance rules | FD table described as global map keyed by `(task_id, fd)` | **Fix ownership semantics.** A task-scoped FD table/object model is preferable to a global lookup convention. |
| Process metadata | PID, parent, state, CPU time, memory, capabilities | PID/PPID and scheduler state exist; complete process inspection surface is planned | Add minimal task enumeration for `ps` and diagnostics. |
| Signals/events | Explicit async event or signal model | No general signal delivery | Do not add a large POSIX signal surface for v0.1. Provide notifications/termination first. |
| Job control | Foreground process group, background task control | Not established | Defer until terminal and process groups exist. |
| Resource limits | Per-process memory, handle and CPU limits | Not established as a complete contract | Design limits early; implement basic guardrails before calling the OS secure. |

### Recommended native process contract

For v0.1, prefer a small native lifecycle:

```text
spawn(executable, argv, envp, inherited_handles, capabilities) → process_id
wait(process_id) → exit_status
exit(exit_status)
getpid() → process_id
getppid() → parent_id
```

This provides enough functionality for a shell to execute external commands. It avoids taking on `fork()`, `execve()`, process groups, signals, and full POSIX semantics prematurely.

---

## 5. Filesystem, storage, and volume management

DOS's most durable product idea was the uniform file interface. A modern system should extend that abstraction rather than inherit the old filesystem limitations.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| VFS | One namespace for filesystems, devices, pseudo-filesystems and remote mounts | VFS, inode interface, mount table and longest-prefix matching described | Keep; formalize mount/unmount and inode operation contracts. |
| Persistent filesystem | One well-tested default filesystem with journaling/recovery; additional FS drivers as modules | FAT32 read/write tested; ext4 daemon has substantial partial implementation but acceptance gaps | Pick one guaranteed filesystem for v0.1. Treat ext4 as experimental until crash and interoperability tests pass. |
| FAT support | FAT32/exFAT compatibility for removable media and boot interchange | FAT32 with 8.3 + LFN entries and Linux interop described | Keep FAT32 as a practical compatibility target; do not make it the only long-term filesystem. |
| Filenames | Long Unicode-capable names, case-handling rules and explicit normalization | FAT32 LFN support documented | Define case/encoding rules in VFS; test edge cases. |
| File operations | Create/open/read/write/seek/close/stat/rename/unlink/truncate/fsync | Core open/read/write/seek/close and basic mutations exist; stat/rename/access are listed as missing | Add missing metadata and durable-write operations needed for a stable ABI. |
| Directories | Hierarchical directories, enumeration, current working directory, path normalization | Path lookup, getdents and mkdir/rmdir/unlink described; cwd API incomplete | Add `getcwd`/`chdir` or an explicit shell-managed cwd contract. |
| Metadata | Timestamps, file type, size, permissions/ACLs, ownership, flags | FAT timestamps use LAPIC tick counter; permission model is incomplete | Do not use scheduler ticks as a real wall clock. Add clock-backed timestamps and define security semantics. |
| File sharing | Explicit locks and sharing policies | Not a complete documented feature | Defer advanced record locking; never promise lock semantics until tested. |
| Caching | Unified page cache, read-ahead, writeback, flush and cache coherency | 256-page cache allocation described; filesystem paths do not use it yet | Integrate only after correctness tests; cache coherency and writeback are release-quality concerns. |
| Journaling | Transactional metadata updates and crash recovery | ext4 JBD2 code described; interrupted-write replay/e2fsck tests pending | Do not advertise ext4 recovery until tests demonstrate it. |
| Block devices | Common block-device API, queueing, flush, discard/TRIM, errors | VirtIO-blk Ring 0 driver and VFS block inode described | Keep VirtIO as the v0.1 reference device; define block errors and flush semantics. |
| Partitioning | GPT and protective MBR, partition discovery | Full partition manager not established in reviewed docs | Later milestone. A single test volume is enough for initial MVP. |
| Volume management | Mount, unmount, labels, free space, read-only mounts | Mount table and automatic mount are described | Add read-only/recovery mounts and a `df`-style tool. |
| Disk utilities | Format, check, repair, copy, backup/restore, usage reporting | Format syscall for ext4 described; CLI and robust filesystem check are incomplete | Keep formatting behind strong admin capabilities; add check/interop tooling later. |
| Encryption | At-rest encryption and key management | Not documented | Defer; do not roll your own cryptography. |
| Snapshots/quotas | Optional advanced filesystem capabilities | Not implemented | Not a v0.x target. |

### Recommended filesystem policy for v0.1

- Declare **one filesystem path as supported**, based on tests, not ambition.
- FAT32 is currently the clearest documented interoperability candidate.
- Keep ext4 marked experimental until interrupted-write replay, malformed metadata rejection, and Linux `e2fsck`/mount interoperability have acceptance evidence.
- Do not call a filesystem "stable" because create/read/write works once in QEMU.
- Require a reboot persistence test and a host-side image inspection test.

---

## 6. Device model, drivers, and hardware discovery

A modern DOS could retain the notion of simple logical devices such as console, null, serial, and storage — but use explicit device objects and capability-controlled access.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Device namespace | `/dev`-style names or typed device handles for console, null, storage, input, audio, network | `/dev/sda`, stdio inode and console paths described; `/dev/null`/`zero` are on checklist | Add stable standard devices; make device handles obey the same ownership model as files. |
| Driver framework | Driver manager, device enumeration, binding, resource grants, lifecycle and restart | Driver Manager and Ring 3 driver server framework are planned | This is a defining Serix milestone, but should follow resource-authority and DMA design. |
| Character devices | Console, serial, keyboard, mouse, printer-like output, event streams | PS/2 keyboard and framebuffer/serial console paths | Standardize input/output as devices; preserve serial for recovery. |
| Block devices | VirtIO, NVMe, SATA/AHCI, USB mass storage | VirtIO-blk Ring 0 driver | Keep VirtIO as the QEMU baseline; NVMe and USB storage later. |
| PCIe | Enumeration, BAR/resource allocation, MSI/MSI-X, hotplug, reset and error reporting | PCI enumeration and bounded BAR/MSI-X metadata discovery; no full resource manager or hotplug | Complete resource ownership before adding broad device support. |
| USB | xHCI host controller, HID, mass storage, hotplug | xHCI discovery metadata; no controller driver; PS/2 input in Ring 0 | Defer until driver-server framework and DMA safety exist. |
| Input | Unified keyboard, pointer, gamepad and hotplug event model | PS/2 input only | Add input service and event ABI after device framework. |
| Display | UEFI GOP/framebuffer first, terminal, later GPU acceleration | Limine framebuffer console / graphics crate | Keep framebuffer as baseline. GPU acceleration is out of v0.1 scope. |
| Audio | HDA/USB audio, device graph and user-space mixing service | Not documented | Defer. |
| Network | Ethernet/Wi-Fi drivers plus independent network stack/service | No network driver, NetDaemon or TCP/IP stack | Separate milestone; do not pretend networking exists because a NetDaemon is on the roadmap. |
| Firmware/platform | ACPI device discovery, power, thermal and RAS interfaces | ACPI RSDP/root/MADT/FADT/DMAR parsing described; power control and IOMMU not wired | Keep discovery; implement power and DMA isolation as separate validated projects. |
| Virtual devices | VirtIO devices for deterministic emulation and testing | VirtIO-blk | Prefer VirtIO for early test coverage; add hardware-specific devices only with a test plan. |

### Recommended driver architecture

```text
                 Ring 3
┌──────────────────────────────────────────────────────┐
│ init / Service Manager                               │
│                                                      │
│  Device Manager                                      │
│     ├── Block driver service                         │
│     ├── Input driver service                         │
│     ├── Network driver service                       │
│     └── Other device services                         │
│                                                      │
│  Filesystem services / shell / applications          │
└───────────────────────┬──────────────────────────────┘
                        │ capability-authorized IPC
                        ▼
                 Ring 0 kernel
┌──────────────────────────────────────────────────────┐
│ Syscall ABI · IPC · Scheduler · VM · Capability core │
│ Interrupts · PCI resource authority · DMA/IOMMU API   │
└───────────────────────┬──────────────────────────────┘
                        │ controlled MMIO / DMA
                        ▼
                 Hardware / QEMU
```

**Do not expose arbitrary physical addresses or unrestricted DMA buffers to a driver process.** A Ring 3 driver is only meaningfully isolated if the kernel controls its MMIO mapping, interrupt assignment, DMA buffers, device reset path, and authority to access the device. IOMMU-backed isolation is the target for DMA-capable devices; until it exists, document the weaker trust model honestly.

---

## 7. System-call ABI and I/O model

DOS's INT 21h was valuable because it gave applications a hardware-independent interface. Serix should provide the same conceptual benefit with a stable native ABI, not copy the interrupt numbers or register conventions.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Syscall entry | Architecture-specific fast entry, versioned ABI, stable error convention | `SYSCALL`/`SYSRET` and Serix-native syscall numbers | Document and freeze the v0.1 ABI subset. Keep syscall implementation details private. |
| File I/O | Open/read/write/seek/close/stat/rename/unlink/fsync | Open/close/read/write/seek/dup/dup2/pipe/getdents and basic mutation calls described | Fill critical gaps; define FD and capability semantics. |
| Standard streams | stdin/stdout/stderr as handles to console or redirection targets | FDs 0–2 are described as PS/2 keyboard, framebuffer console, and serial | Normalize stdio behavior; support redirection through FD duplication, not shell-only hacks. |
| Pipes | Kernel-managed bounded pipe with blocking/non-blocking semantics and EOF | `SYS_PIPE` described; shell output redirection exists | Test producer/consumer, close semantics, backpressure, pipe capacity and deadlock behavior. |
| Device control | Typed ioctls or per-device control objects | `ioctl` is on the v1 checklist, not part of the documented syscall list | Add only when there is a concrete device need; avoid one untyped catch-all API. |
| Memory API | `mmap`, `munmap`, anonymous allocation, heap growth | `mmap`/`munmap` described; `brk` and some mapping work remain open | Choose a documented user allocator contract; `brk` is optional if an alternative is well-defined. |
| IPC | Message passing, notifications, shared memory, handle transfer | Queued port IPC, blocking receive, notifications, capability checks described | Stabilize message ABI, buffer lifetime, cancellation, sender identity, and handle transfer. |
| Errors | Consistent negative errno-like values or typed errors | Negative errno values encoded in syscall return values | Freeze the mapping and publish a single source of truth for userspace. |
| ABI version | Kernel ABI version query and explicit compatibility promise | Version string in `uname` is on checklist | Add ABI version discovery before promising binary compatibility. |
| Compatibility | Optional POSIX layer; no need to make the native ABI POSIX | Native Serix ABI; Linux compatibility is not the goal | Keep native ABI clean. Add compatibility shims only when a real application needs them. |

### v0.1 ABI scope

Freeze only the calls required for:

- process spawn, wait, exit, PID and PPID;
- open, close, read, write, seek, duplicate and directory enumeration;
- create/remove directories and unlink files;
- pipe creation and use;
- IPC send/receive/notification;
- mmap/munmap if exposed to applications in the release;
- process/system information required by diagnostics;
- policy reload only for authorized administration.

For every call, document: arguments, return values, errors, blocking behavior, ownership, capability requirements, pointer validation, and whether the behavior is stable or experimental.

---

## 8. Shell, command language, and userland utilities

This is where Serix can most visibly preserve the best of DOS.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Default shell | Replaceable Ring 3 shell, not part of the kernel | `kshell` is kernel-resident; Ring 3 `rsh` is planned | **P0 for v0.1.** Make `rsh` the normal interactive shell and retain `kshell` as rescue/debug. |
| Internal commands | Small core builtins for shell control and essential diagnostics | `kshell` builtins include help, echo, ls, cat, write, mkdir, rm, mount, umount, halt, reboot; newer docs include logs/policy/clear | Port only essential builtins to `rsh`; don't grow two competing shells. |
| External commands | Each command is a separate executable | External command execution is planned | **P0 for v0.1.** `rsh` must spawn a static ELF from VFS and collect its exit status. |
| Redirection | stdin/stdout/stderr redirection via handle duplication | Kernel shell supports `>` and `>>` | Move redirection to FD operations and test actual user processes. |
| Pipelines | `cmd-a | cmd-b` using kernel pipe objects | Pipe syscall exists; pipeline support is not documented as complete | Implement after external commands; do not use temporary disk files. |
| Wildcards | Shell glob expansion or explicitly documented command-level globbing | Not established | Implement one consistent expansion rule later. |
| Environment | Variables, PATH, HOME, TEMP, locale | Shell variables are noted in roadmap; process env ABI incomplete | Implement PATH and argv/envp in spawn before advanced scripting. |
| Scripting | Readable scripts with variables, conditionals, loops and exit status | Existing shell parser has variables, `if`, `repeat`, tokenizer and history; userland port incomplete | Preserve the language but move it to Ring 3 and publish syntax. |
| Interactive editing | Cursor movement, history, basic completion | Command history exists; line editor is incomplete | Add reliable input editing; completion can follow. |
| Job control | Foreground/background jobs, job listing and cancellation | Not established | Defer until process groups and terminal ownership exist. |
| Package/tool model | Small system utilities and signed packages/repository metadata | No package manager documented | Start with a curated set of statically linked system commands. Package manager is later. |
| Recovery shell | Minimal shell available when services fail | Kernel shell is already available | Keep and harden it as the recovery console. |

### Suggested initial command set

| Command | Purpose | v0.1 status |
|---|---|---|
| `help` | Show commands and usage | Must have |
| `echo` | Print text | Must have |
| `ls` | List directory contents | Must have |
| `cat` | Read and print a file | Must have |
| `pwd` / `cd` | Working directory | Must have, with a defined cwd model |
| `mkdir` / `rmdir` | Directory operations | Must have |
| `touch` | Create an empty file | Should have |
| `rm` | Remove a file | Must have |
| `cp` / `mv` | Copy/move files | Can follow once copy/rename are tested |
| `mount` / `umount` | Mount management | Must have basic form if supported by VFS |
| `ps` | Inspect tasks | Must have basic form |
| `mem` | Memory overview | Should have |
| `sysinfo` | Kernel, CPU and memory summary | Should have |
| `dmesg` / `logs` | View kernel log | Must have a usable path |
| `df` | Show mounted volume usage | Should have |
| `poweroff` / `reboot` | Controlled shutdown/restart | Must have for a release image, or be clearly marked unsupported |

### Broader command and utility inventory

The following is the modern equivalent of the broad DOS utility ecosystem. It is a capability catalogue, not a requirement to ship every command in v0.1. A small set of commands should be external programs; only shell-control operations need to be builtins.

| Historical DOS utility family | Modern command/tool equivalents | Serix treatment |
|---|---|---|
| File listing, copy, move, delete and attributes (`DIR`, `COPY`, `XCOPY`, `DEL`, `REN`, `ATTRIB`, `TREE`) | `ls`, `cp`, `mv`, `rm`, `stat`, `chmod`/`chflags` if supported, `tree`, `find` | Basic file commands in v0.1; permissions/attributes only after a real metadata/security model exists. |
| Text display and editing (`TYPE`, `EDIT`, `EDLIN`) | `cat`, `less`, `edit` | `cat` in v0.1; pager/editor later. |
| Disk and volume management (`FORMAT`, `FDISK`, `LABEL`, `CHKDSK`, `SCANDISK`, `DEFRAG`, `DISKCOPY`, `DISKCOMP`) | `disk`, `part`, `mkfs`, `fsck`, `label`, `df`, `du`, `sync`, `blkid` | `mount`, basic format/check tooling as authorized; full partition and repair tooling later. |
| System inspection (`MEM`, `VER`, `DATE`, `TIME`, `VOL`) | `mem`, `sysinfo`, `uname`, `uptime`, `date`, `clock`, `df`, `mount` | `sysinfo`, `mem`, `uptime` and `mount` are the first useful set. |
| Process and resident utilities (`TASKLIST`-style tools, TSR utilities) | `ps`, `top`, `kill`, `service`, `jobs` | `ps` in v0.1; job control and `top` later. |
| Startup and environment (`SET`, `PATH`, `PROMPT`, `AUTOEXEC.BAT`) | `set`, `env`, `export`, `source`, service manifests | PATH/env and init configuration in v0.1; richer scripts later. |
| Shell helpers (`DOSKEY`, `CHOICE`, `CALL`, `IF`, `FOR`, `GOTO`, `SHIFT`) | `history`, `alias`, `read`, `if`, `for`, functions/subcommands | Preserve the existing small script language, document it and keep it in Ring 3. |
| Device and diagnostics (`MODE`, `DEBUG`, `MEM`, `MSD`) | `devctl`, `lspci`, `lsusb`, `dmesg`, `trace`, `debug` | `dmesg` and read-only device inventory first; `devctl` when a device manager exists. |
| Backup/recovery (`BACKUP`, `RESTORE`, `UNDELETE`, `UNFORMAT`) | `backup`, `restore`, `fsck`, recovery shell | Recovery shell now; reliable backup and repair later. |
| Networking (`INTERLNK`, `INTERSVR`, redirectors) | `ip`, `ping`, `ifconfig` equivalent, `route`, `dns`, `netstat`, `curl` | Defer until network device, stack and socket/IPC ABI exist. |
| Programming tools (`DEBUG`, `MASM`, `LINK`, `QBASIC`) | Cross-compiler SDK, assembler, linker, debugger, examples | Support external Rust/LLVM toolchains and a documented SDK; no in-OS compiler requirement. |
| Version compatibility (`SETVER`) | Per-program compatibility metadata or explicit compatibility runtime | Not a native v0.x feature; avoid global version spoofing. |

---

## 9. Startup services, system policy, and security

A modern general-purpose OS can remain easy to administer without relying on ambient access or a globally trusted driver model.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| User identity | Single-user by default; identity model can grow to multiple users | Multi-user identity model is not documented as complete | Keep v0.1 single-user, but do not equate single-user with unrestricted privilege. |
| Capabilities | Explicit authority to access files, processes, IPC, devices and memory | Per-task capability spaces, generation handles, delegation/revocation and audit ring described; some syscall paths remain incomplete | **P0.** Close all authority gaps before describing the model as comprehensive. |
| Policy | Declarative policy with clear precedence and predictable failure behavior | LES path allow/deny policy and `policyd`; default-allow behavior is documented | Resolve fail-open semantics. Protected operations must not silently become allowed when the policy service is unavailable. |
| Audit | Bounded kernel event log with userspace inspection and pagination | Bounded audit ring and `/proc/serix/cap-audit`; pagination is pending | Add pagination, truncation reporting and tests for denied/allowed operations. |
| Driver authority | Device, MMIO, IRQ and DMA capabilities | No general userspace driver resource delegation | Design resource capabilities before Ring 3 hardware drivers. |
| Process authority | Spawn/terminate/wait authority and parent-child constraints | Process control relies partly on task/parent identity | Explicitly authorize process creation and management. |
| Filesystem permissions | Ownership, mode bits, ACL or capability-derived access | POSIX DAC bridge is planned | Do not claim POSIX permissions until semantics and enforcement exist. |
| Secure updates | Signed updates, rollback protection, recovery path | Not documented | Defer. Establish reproducible release artifacts first. |
| Disk encryption | Hardware-backed or passphrase key management | Not implemented | Defer; integrate a proven cryptographic design only when needed. |
| Sandboxing | Per-process authority, resource limits, restricted service interfaces | Capability framework is the foundation | Make capability inheritance and service grants explicit. |

### Security release rule

Do not market LES or the capability subsystem as a security boundary until the negative tests prove that unauthorized access is rejected across every exposed path. A permissive default may be useful during bring-up, but it must be labelled as development behavior and must not be confused with least privilege.

---

## 10. Networking and remote resources

A modern general-purpose system would have networking as an ordinary subsystem, not a collection of TSRs and global interrupt hooks.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Network device | Ethernet and Wi-Fi drivers, plus VirtIO-net for testing | No network driver or NetDaemon | Phase after v0.1. |
| Protocol stack | IPv4/IPv6, ARP/NDP, ICMP, UDP, TCP | No TCP/IP stack | Build as a userspace service/library with explicit buffer ownership. |
| Configuration | DHCP, static configuration, DNS resolver | Not implemented | Later milestone. |
| Sockets | Native socket API or message endpoint abstraction | No network-facing userspace ABI | Design around capabilities and IPC; POSIX socket compatibility is optional. |
| Network filesystems | Remote filesystem mounted through VFS | No network redirector | Add only after stable local VFS and network transport. |
| Remote printing | Print service backed by network transports | Not implemented | Not a v0.1 concern. |
| Firewall | Policy-controlled packet filtering and per-service network authority | LES exists for paths, not a network policy system | Add network policy with the network stack, not as a speculative kernel feature. |
| TLS/security | Mature TLS library, certificate store, system trust roots | Not implemented | Use established libraries and a userspace design; never invent cryptography. |

Networking should be a later milestone. A network stack adds drivers, DMA, buffers, concurrency, timers, protocols, security and user-facing APIs at once. It is a poor v0.1 distraction while the userland execution path is still missing.

---

## 11. Time, locale, power, and platform services

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Wall clock | RTC/firmware-backed wall clock, timezone and UTC handling | LAPIC tick-based file timestamps documented; complete wall clock not established | Add a clocksource and wall-clock API; never treat scheduler ticks as calendar time. |
| Monotonic time | Stable monotonic clock for timers and profiling | LAPIC timer is present; general API is incomplete | Add monotonic time and sleep APIs. |
| Locale | UTF-8, Unicode, locale-aware date/number formatting | Not established | Keep locale outside kernel; begin with UTF-8 and clear text rules. |
| Keyboard layout | Layout mapping in input service | PS/2 scan-code path | Move layout mapping to userspace input service eventually. |
| Power states | ACPI shutdown, reboot, suspend/resume, CPU idle and thermal controls | ACPI FADT parsing; power controls and richer C/P-states are pending | At minimum implement tested poweroff/reboot. Suspend/resume is later. |
| Battery | Battery status and power-source events | Not implemented | Laptop feature milestone. |
| Hybrid CPU power | E/P-core topology and performance/energy hints | Core-type detection infrastructure described | Defer policy until scheduler and thermal foundations are validated. |
| Thermal/RAS | Temperature sensors, throttling, machine-check/error reporting | Not a complete implemented subsystem | Later hardware reliability work. |

---

## 12. Diagnostics, recovery, and system maintenance

DOS was useful because users could operate and repair a machine from the command line. Serix should treat observability and recovery as core operating-system features.

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Kernel log | Structured ring buffer, severity, timestamps, subsystem names | Serial/framebuffer logging and audit ring described | Add a stable log buffer readable from userspace. |
| Process inspector | PID, state, parent, CPU time, priority, memory, handles | Task structures and PID calls exist; `/proc/[pid]` data planned | Basic `ps`/task table is a v0.1 usability requirement. |
| Memory inspector | Total/free/used frames, heap, mappings, page cache | Some allocator/VMA/cache telemetry exists; `/proc/meminfo` planned | Add accurate counters only after allocator accounting is correct. |
| Device inspector | PCI IDs, driver binding, resources, IRQ, state | PCI inventory and ACPI tables described | Add read-only device inventory first; do not imply resource manager exists. |
| Filesystem checker | Consistency scan and explicit repair mode | FAT32 operations and ext4 work; complete checker not documented | Defer full repair tools; add host interoperability tests now. |
| Crash reporting | Panic dump, registers, fault address, task ID and log tail | Panic serial output with register dump described | Preserve and test; add user-process fault isolation and crash reporting. |
| Debugger | Kernel symbols, serial/GDB path, breakpoints and tracepoints | QEMU GDB workflow documented | Keep developer debugging separate from user-facing v0.1 tools. |
| Backup/restore | File-level backup and verified restore | Not implemented | Later; not a v0.1 release gate. |
| Storage health | SMART/NVMe health and errors | No NVMe driver | Later hardware enablement. |
| Automated tests | Boot smoke tests, syscall tests, filesystem tests, stress tests | Repository docs say no automated test suite / CI remains planned | **P0.** A release must be reproducibly testable. |

### Minimal pseudo-filesystem proposal

Implement a deliberately small information surface rather than cloning all of Linux `/proc`:

```text
/proc/serix/version
/proc/serix/uptime
/proc/serix/cpuinfo
/proc/serix/meminfo
/proc/serix/tasks
/proc/serix/cap-audit
/dev/console
/dev/null
/dev/zero
```

Per-process files can be added when the process table and lifecycle semantics are stable. Pseudo-files should be read-only unless a specific control operation requires writes.

---

## 13. Graphics, applications, compatibility, and developer tools

| Feature area | A modern general-purpose OS borrowing DOS's product philosophy | Serix today | Serix direction |
|---|---|---|---|
| Text UI | Fast framebuffer/terminal shell, Unicode fonts, resize-aware console | Framebuffer and serial consoles | Keep text UI first; it is ideal for early OS bring-up. |
| GUI | Compositing desktop, window server and graphics drivers as a normal supported product mode | No full GUI/windowing system | Not v0.1, but a required long-term pillar for desktop/general-purpose credibility. Build only after process, memory, input and display contracts are reliable. |
| GPU | Display, command submission and memory management through a driver stack | No GPU driver framework | Much later. |
| Audio | Device service and user-space mixer | Not implemented | Later. |
| Static apps | Native ELF64 binaries linked against `ulib` | Static ELF64 loading described | Core v0.1 app model. |
| Dynamic linking | ELF interpreter, shared objects, relocations, TLS and auxv | `PT_INTERP`/dynamic linker not implemented | Defer until static applications are reliable. |
| Toolchain | Compiler, assembler, linker, debugger, SDK and cross-build support | Rust workspace and build scripts | Document an SDK/build recipe; shipping an in-OS compiler is not needed. |
| Package management | Signed package manifests, dependency resolution, rollback | No package manager documented | Later. Start with a reproducible system image and a small app directory. |
| DOS compatibility | Optional virtual machine or compatibility environment | Not a stated objective | Do not make it a core objective. |
| POSIX compatibility | Optional compatibility layer for selected APIs | Native Serix syscall ABI | Add only when concrete software demand justifies the cost. |
| Linux binary compatibility | Optional advanced subsystem | Explicitly not the current goal | Not a v0.x goal; preserve architectural room for a future decision. |
| Legacy 8.3 names/FCBs | Optional import/export compatibility | FAT32 LFN support; no FCB ABI | Do not reproduce FCBs, PSP or MCBs in the native design. |
| Developer introspection | `sysinfo`, `ps`, `mem`, `mount`, `dmesg`, `devctl`, tracing | Some kernel shell commands exist | Make tools consume public kernel interfaces rather than reaching into globals. |

A command-line environment is the right first interface for bringing up and diagnosing Serix, but it is not the end-state product. A credible general-purpose OS needs a graphical stack, application runtime, software distribution and a useful set of native applications. Those are staged roadmap commitments, not features to cram into v0.1.

# Part II — What this means for Serix

## 14. DOS concepts translated into a general-purpose Serix architecture

| DOS idea worth retaining | Modern Serix design | Linux-class maturity requirement | Horizon |
|---|---|---|---|
| Replaceable command interpreter | Ring 3 shell, independent commands and documented shell ABI | Multiple shells/tools, terminal sessions, scripting and robust job control | v0.1 foundation; grow continuously |
| External command execution | Process creation, ELF loader, VFS lookup, argv/envp and wait/exit | Complete process lifecycle, signals, threads, resource limits and dynamic runtime | v0.1 then v0.x |
| Uniform file/device I/O | VFS, typed kernel objects, per-process handles and device namespace | Stable semantics, permissions, poll/event support, metadata, locking and network resources | v0.1 baseline; mature over several releases |
| Standard streams and redirection | Descriptor inheritance/duplication and console/pipe objects | PTYs, terminal modes, pipelines, job control and robust backpressure | v0.1 baseline; extend |
| Installable drivers | Resource broker, driver manager, capability grants and restartable driver services | PCI resource management, interrupt and DMA isolation, IOMMU, hotplug, power and broad driver coverage | v0.2 onward |
| CONFIG.SYS / AUTOEXEC.BAT | Validated boot config, init/service manifests and scripts | Dependency graph, service health, restart policy, user sessions and update integration | v0.1 basic init; grow |
| FAT interoperability | VFS filesystem drivers and explicit supported-format matrix | Reliable journaling, consistency checks, permissions, ACLs, encryption and recovery | FAT32 now; ext4 validation and more later |
| DOS memory allocation | Per-process virtual memory, page faults, mappings, reclamation and accounting | Complete VM, swapping policy if desired, memory pressure, NUMA and security hardening | Correctness now; mature later |
| TSR/background work | Supervised services, IPC, notifications and scheduler contracts | Resource controls, restart policy, service dependencies and lifecycle guarantees | v0.1 basic services; grow |
| Network redirector | Network filesystem integrated through VFS and network services | Full network stack, sockets, DNS, TLS, firewall, remote mounts and diagnostics | v0.3+ |
| DOS diagnostics | `ps`, `mem`, `sysinfo`, `dmesg`, `mount`, device and policy inspection | Complete observability, tracing, crash dumps, telemetry controls and recovery tools | v0.1 basic set; grow |
| DOS error handling | Versioned syscall ABI, typed errors and safe user-memory validation | Consistent error semantics across kernel, drivers, services and libraries | v0.1 baseline; freeze only after use |
| DOS version compatibility | Serix ABI versioning and compatibility policy | Stable ABI, migration rules and compatibility suite | v0.1 policy; v1.0 stability contract |
| Simple administration | Native configuration and text-first recovery | GUI settings, system installer, update/rollback, user management | Stage by stage |

## 15. What Serix must not inherit from DOS — and what it must not oversimplify

| Temptation or legacy limitation | Serix rule |
|---|---|
| Real-mode memory and segmentation model | Use long mode, virtual memory and protected address spaces. |
| Single shared address space | Isolate Ring 3 processes and validate every boundary. |
| Single-tasking kernel | Retain preemptive SMP, then prove scheduler correctness under contention and failure. |
| Unprotected Ring 0 drivers | Minimize trusted kernel code; delegate hardware resources only through enforceable MMIO/IRQ/DMA/IOMMU controls. Do not move drivers to Ring 3 before the resource model is safe. |
| TSR interrupt-hook extension model | Use explicit services, IPC, events, supervision and lifecycle management. |
| Global mutable device/file state | Use typed objects, task-owned handles, capability authority and synchronized ownership. |
| 8.3-only path constraints | Keep long filenames, Unicode policy and well-defined path normalization. |
| Disk-backed shell pipes | Use bounded kernel pipe objects with EOF, error and backpressure semantics. |
| Ambient global access | Require explicit capabilities and well-defined inheritance/delegation. |
| BIOS as the universal hardware API | Use firmware for boot/discovery and native platform drivers after handoff. |
| No resource cleanup | Require teardown for tasks, mappings, handles, IPC ports, DMA and device resources. |
| Implicit failure behavior | Specify errors, timeouts, cancellation, recovery and crash containment. |
| “Everything belongs in the kernel” | Keep policy, filesystems, drivers, network protocols and user-facing services outside Ring 0 where practical. |
| “Everything must be in userspace” dogma | Keep only latency-critical or protection-critical mechanisms in the kernel; justify each boundary with measurement and threat model. |
| Compatibility as an afterthought | Define ABI/versioning and portability policy early, but implement compatibility only against real application targets. |
| A shell as the entire OS | Treat shell as the first admin/developer interface, not the final application environment. |

## 16. Current Serix snapshot: strengths and unfinished contracts

This assessment is based on the repository documentation and the `dev` snapshot reviewed on 13 November 2026. It is not a fresh execution of the kernel or a code-level proof of every documented claim.

### Existing foundation worth consolidating

| Area | Repository evidence | Assessment against the long-term objective |
|---|---|---|
| Boot | Limine BIOS/UEFI flow, higher-half kernel, HHDM and framebuffer | Good controlled platform baseline; eventually needs installer, secure boot path and broader firmware/hardware testing. |
| CPU topology | CPUID parsing, topology and hybrid-core classification infrastructure | Relevant to modern heterogeneous CPUs; mature only after scheduler and power-policy evidence. |
| Interrupts/SMP | LAPIC, I/O APIC, x2APIC path, per-CPU state, run queues and IPI mailboxes | Substantial low-level base; stress, race and fault testing remain more important than new scheduler features. |
| Scheduler | Preemptive per-CPU WFQ with priority inheritance | Keep the current policy until correctness/performance data justifies change. |
| IPC | Queued ports, blocking receive, notifications and direct-message fast path | Core hybrid-kernel primitive; needs ABI discipline, lifetime/teardown rules, abuse tests and performance evidence. |
| Process launch | Static ELF loading and `spawn`/`wait`/`exit` path described | Enough to build initial independent native applications; not yet a complete application runtime. |
| Capabilities | Per-task CSpace, generation references, delegation/revocation and audit | Strong architectural direction; enforcement gaps and fail-open behavior must be closed before security claims. |
| VFS | Inode abstraction, longest-prefix mount table, RAM nodes and block inode | Good base for local, removable and eventually remote resources. |
| FAT32 | Read/write, directories, LFN, cluster allocation and host interoperability described | Useful first persistent filesystem, but durability, crash behavior and edge-case tests need formal acceptance. |
| ext4 | Ring 3 daemon, extents, JBD2 and HTree work described | Strategic filesystem work, but recovery/interoperability/corruption tests are still gating. |
| Policy service | Ring 3 `policyd` and LES bridge | Demonstrates service separation; its documented default-allow/fallback behavior needs an explicit security redesign. |
| Hardware inventory | PCI enumeration and ACPI table parsing | Discovery is not resource management, hotplug, IOMMU isolation or driver support. |

### Gaps that prevent Serix from becoming a coherent OS

| Gap | Why it matters to a Linux-alternative ambition | Recommended treatment |
|---|---|---|
| Normal shell is still kernel-resident | No proper userland boundary in the default interactive path | Port `rsh` to Ring 3; retain `kshell` for diagnostics/recovery only. |
| External command environment incomplete | A useful OS needs independently executable programs and libraries | Complete argument/environment passing, command lookup, wait status and clean child teardown. |
| Init/service management incomplete | Real operating systems must start, monitor, restart and stop services predictably | Build explicit Ring 3 init/service manager and service manifests. |
| Process and FD ownership contracts need closure | Leaks and cross-task confusion undermine every application | Formalize per-process FD tables, inheritance, close-on-exit and teardown. |
| CPU extended-state management needs validation | FPU/SSE state corruption can break user applications silently | Treat FPU/SSE/XSAVE save/restore and context isolation as a hard kernel gate. |
| Physical frame reclamation and VM lifecycle | Without it, process churn and mappings can exhaust memory | Implement safe reclamation, page-table teardown and TLB invalidation/shootdown before reuse. |
| Page cache disconnected from filesystem paths | Allocating a cache does not create coherent caching or writeback | Integrate with coherence/writeback tests or do not advertise it as functional. |
| Driver resource model absent | Ring 3 filesystem daemons do not prove user-space driver isolation | Specify resource broker, MMIO, IRQ, DMA and IOMMU ownership before migrating drivers. |
| Networking absent | No network services or network-capable app ecosystem | Add a network milestone after a stable local system and driver resource model. |
| No real hardware matrix | QEMU success is necessary but insufficient | Maintain a tested hardware list and expand one device class at a time. |
| No GUI/windowing stack | Serix cannot be a general desktop alternative without graphical applications | Plan display, input, compositor/window server, graphics API and applications as long-term pillars. |
| No package/update ecosystem | Users cannot install, update or recover an OS sustainably | Design signed/reproducible system images and packages, atomic update/rollback and recovery. |
| Security defaults and authorization gaps | A capability OS must demonstrably deny unauthorized operations | Close every syscall/resource path, test policy-service failure and avoid privileged fail-open defaults. |
| No automated release suite | Regression-prone low-level work cannot scale by manual QEMU checks | Build CI, deterministic QEMU tests, stress tests, filesystem fault tests and eventual hardware CI. |

### Documentation/version discrepancies to resolve

1. The user-provided current released baseline is `v0.0.8`.
2. The reviewed `dev` branch roadmap labels itself `v0.0.9` and includes more recent work.
3. The reviewed architecture document still identifies `v0.0.8`.
4. The top-level README presents subsystem status but should carry a synchronized release/version statement.
5. `docs/CHECKLIST_v1.md` is a v1.0 checklist; it should not be treated as the v0.1 scope.
6. TSS.RSP0/context-switch descriptions differ across architecture/checklist material. Verify the implementation and update all references from one source of truth.
7. Status labels must distinguish “code exists”, “tested in QEMU”, “tested on real hardware”, “interoperable”, and “production-grade”. These are not interchangeable.

## 16A. What “credible Linux alternative” means

A credible alternative is measured by complete user outcomes, not by subsystem count. The following matrix is the long-range product scorecard. It is not a v0.1 checklist.

| Product pillar | Minimum credible outcome | Evidence required | Current position |
|---|---|---|---|
| Installation and boot | A user can install or deploy Serix on supported machines, boot reliably and recover from failed updates | Installer/image builder, UEFI matrix, boot/recovery test suite | Limine/QEMU foundation; installer/recovery product incomplete |
| Hardware support | Common storage, USB input, network, display, audio and power work on a declared device matrix | Published compatibility matrix, real-hardware test logs, driver lifecycle tests | Early discovery and VirtIO/PS2 paths; broad driver support absent |
| Process/runtime | Isolated processes, threads, dynamic linking, libraries, signals/events, resource controls | ABI tests, stress tests, application compatibility tests | Early static ELF process model |
| Storage | Reliable local storage with integrity checks, permissions, recovery and removable/partitioned media | Crash/fault injection, filesystem interoperability, fsck/repair tests | FAT32 and partial ext4 paths; validation gaps remain |
| Networking | Ethernet/Wi-Fi, IPv4/IPv6, TCP/UDP, DNS/DHCP, TLS, firewall and sockets | Protocol conformance, security tests, real NIC matrix | Not implemented in reviewed snapshot |
| Security | Isolation, least privilege, access control, secure update and useful audit/recovery | Negative tests, threat model, vulnerability response process | Capability/LES foundation with enforcement gaps |
| Desktop experience | Display, input, compositor/window server, graphics acceleration path, audio and usable apps | Interactive desktop test matrix and app demonstrations | Not implemented |
| Server/headless experience | Reliable service manager, network services, logs, storage, remote administration and updates | Long-running soak tests, service recovery and operations tests | Early services only |
| Applications | Native SDK, libraries, package manager and enough applications for useful workloads | App catalogue, packaging pipeline, developer docs and user testing | Native runtime early; ecosystem absent |
| Compatibility | Deliberate POSIX surface and/or selected Linux application compatibility where it helps adoption | Compatibility test suite and supported application list | Strategy not yet finalized |
| Updates and lifecycle | Signed/reproducible updates, rollback, backups, supported releases and migration policy | Reproducible build, signature verification, update fault injection and support policy | Not implemented |
| Performance and reliability | Competitive performance in defined workloads without sacrificing security or stability | Published benchmarks, regressions, stress/soak and fault tests | Insufficient data for claims |
| Ecosystem and governance | Maintained docs, contribution process, security reporting, release cadence and community | Public roadmap, release notes, issue triage and security process | Early project infrastructure |

**The real competitive unit is a working workload on real hardware.** For example: install Serix, connect to a network, update it, run a useful application, store data safely, recover from a crash and administer it. Every roadmap phase should move toward more such complete journeys.

# Part III — Recommended v0.1.0 definition

## 17. What v0.1.0 should mean

`v0.1.0` should be the first **coherent, testable systems milestone** in Serix's path toward a general-purpose operating system. It is not a product-completion milestone and must not be described as a Linux replacement. Equally, it should not be branded as a hobby-kernel demo or a DOS-inspired developer toy.

### Proposed release name

**Serix v0.1.0 — Foundational Systems Preview**

### Release statement

> Serix v0.1.0 demonstrates a bootable x86-64 hybrid operating system with a tested kernel/user boundary, preemptive SMP baseline, native process execution, a Ring 3 operating environment, capability-mediated resource access, and persistent filesystem operations on a declared virtual hardware configuration. It establishes the contracts and test infrastructure on which the broader driver, networking, application, desktop and server roadmap will be built.

This statement is ambitious but bounded. It does not imply broad hardware compatibility, Linux application compatibility, production security certification, a desktop environment or feature parity with Linux.

### What v0.1.0 must prove

| Release promise | Required observable behavior | Why this matters beyond v0.1 |
|---|---|---|
| Boots reproducibly | Clean build and repeatable BIOS/UEFI QEMU boot with 1 and multi-CPU configurations | Establishes the validation foundation for all later drivers and platforms. |
| Has a real userland | Ring 3 init and shell; shell launches independent native programs | Establishes the application boundary instead of making the kernel itself the operating environment. |
| Runs independent processes safely | Arguments, environment, wait/exit, faults and resource teardown behave predictably | Every service, application, package and desktop component depends on this. |
| Provides useful I/O | Standard streams, file handles, pipes/redirection and a declared persistent filesystem path | Establishes the common I/O foundation for utilities and applications. |
| Enforces resource authority | Capability and policy negative tests prove denied operations stay denied | Establishes a security model that can scale to drivers and services. |
| Is inspectable and testable | Logs, basic process/memory/system tools and automated QEMU smoke tests | Makes development and later community contributions reproducible. |
| Has honest limitations | Every supported feature has status and test evidence; unsupported hardware is documented | Prevents marketing claims from outrunning implementation. |

### Explicit v0.1 non-goals — deferred, not abandoned

| Deferred feature | Why it is not a v0.1 gate | Long-term status |
|---|---|---|
| Linux binary compatibility | Requires a stable syscall, signal, memory, threading, dynamic-linker and libc story | Strategic decision after native runtime matures |
| Full POSIX conformance | Large surface; implement against real application requirements | Compatibility workstream |
| General network stack | Requires device I/O, buffers, timers, protocols, sockets and security | Core general-purpose OS pillar, planned after v0.1 foundation |
| Broad Ring 3 driver framework | Must first design safe MMIO/IRQ/DMA/IOMMU authority | Core hybrid architecture workstream |
| NVMe, xHCI, Wi-Fi, audio and GPU breadth | Too many hardware classes at once would dilute correctness work | Essential hardware roadmap, staged by device class |
| GUI/window server/compositor | Depends on stable process, memory, input and graphics interfaces | Required for a desktop-capable end state |
| Dynamic linking and broad application runtime | Static ELF applications are sufficient to validate the first userland slice | Required for mature app ecosystem |
| Package manager and system updater | Needs reliable image formats, trust model and rollback semantics | Required for sustainable OS lifecycle |
| Multi-user desktop and server hardening | Identity, DAC/ACL, sessions, quotas and policy model require deliberate design | General-purpose maturity workstream |
| Suspend/resume and advanced power policy | Requires more complete ACPI/device driver lifecycle | Hardware/laptop maturity workstream |

These items must remain visible on the strategic roadmap. “Not in v0.1” must never silently become “not part of Serix”.

## 18. v0.1.0 release gates — split priority matrix

Every gate below is a release criterion or an explicit blocker. The project should update the implementation status only when the evidence exists.

### Gate A — boot, CPU and kernel correctness

| Requirement | Priority | Acceptance evidence |
|---|---|---|
| Clean Limine boot on the declared QEMU BIOS/UEFI configurations | P0 | Automated boot log with named checkpoints and failure timeout |
| 1-CPU and multi-CPU initialization | P0 | CPU count and per-CPU initialization match QEMU configuration |
| Interrupt and timer stability | P0 | Long-running tick/preemption test with no lost EOI, deadlock or reset |
| Context-switch correctness | P0 | Stress tests covering GPR, flags, CR3, FS/GS and kernel/user transitions |
| FPU/SSE/extended CPU state isolation | P0 | Two processes with distinct floating/vector state do not corrupt one another |
| Frame and address-space reclamation | P0 | Repeated create/map/exit cycles do not leak unbounded frames or reuse live pages |
| Cross-CPU TLB correctness before frame reuse | P0 where multi-CPU reclamation is enabled | Shootdown/acknowledgement tests and stale-mapping stress |
| Bad user pointers and malformed syscalls | P0 | Syscall returns defined error or contains process fault; kernel remains alive |
| Double-fault and panic diagnostics | P1/P0 depending on current exception-path coverage | Dedicated IST stack or documented safe equivalent; serial register dump |

### Gate B — process model and native userland

| Requirement | Priority | Acceptance evidence |
|---|---|---|
| Ring 3 init process owns normal startup | P0 | Boot trace shows init starts and supervises configured core services |
| Ring 3 `rsh` is the normal interactive shell | P0 | Shell runs outside Ring 0; `kshell` is debug/recovery only |
| Launch independent static ELF programs | P0 | Shell resolves command path, spawns child and reports exit status |
| Correct `argv` and `envp` construction | P0 | Test binary prints exact arguments/environment including empty and quoted values |
| Parent-child wait/exit semantics | P0 | Parent observes exit code and zombie/resource reaping is verified |
| Per-process file descriptor ownership | P0 | Child exit closes owned handles; duplication/inheritance semantics are tested |
| Fault containment | P0 | Deliberately faulting user program does not kill kernel, shell or unrelated process |
| Basic service lifecycle | P0 | Init starts mandatory service, records failure and applies explicit restart/fail policy |
| Threads, fork and full signals | P1 / later | Do not implement just to inflate v0.1; document process model and next ABI steps |

### Gate C — storage and I/O correctness

| Requirement | Priority | Acceptance evidence |
|---|---|---|
| Stable per-process handles and standard streams | P0 | stdin/stdout/stderr work across child processes |
| File open/read/write/seek/close | P0 | Automated syscall tests on supported filesystem |
| Directory enumeration and mutations | P0 | `ls`, `mkdir`, `rm`/unlink tests including error paths |
| Pipes and redirection | P0 for advertised shell functionality | Pipe producer/consumer, EOF, buffer-full/backpressure and error tests |
| One declared persistent filesystem path | P0 | Format/mount/create/write/reboot/read/delete sequence succeeds |
| Host interoperability | P0 for the chosen filesystem | Host OS can inspect files produced by Serix without repair |
| ext4 crash recovery claim | Not required if explicitly marked experimental | If claimed, interrupted-write replay and `e2fsck` interoperability tests are mandatory |
| Unified page-cache claim | Only if integrated | FS read/write, mmap coherence and writeback tests; otherwise mark incomplete |
| Device nodes and null/zero/tty | P1 | Deterministic character-device behavior and tests |

### Gate D — capabilities, policy and service boundaries

| Requirement | Priority | Acceptance evidence |
|---|---|---|
| Every exposed resource syscall classified by authorization policy | P0 | Exhaustive syscall-to-rights test; no unclassified dispatch arm |
| Process creation and management authority is explicit | P0 | Unauthorized spawn/wait/control attempts are rejected |
| IPC port authority and lifetime are enforced | P0 | Forged/stale/revoked capability tests; queue lifetime and teardown tests |
| Filesystem mutation requires proper directory authority | P0 | Negative tests for create/unlink/mkdir/rmdir outside granted authority |
| Policy-service failure behavior is operation-specific | P0 | Protected operations fail closed; explicitly public operations are documented |
| Audit events are readable and bounded | P1 | Pagination/truncation indication and denial audit tests |
| Driver MMIO/IRQ/DMA delegation | Later | Do not expose unisolated DMA/MMIO just to claim Ring 3 drivers |

### Gate E — operations, tools and release engineering

| Requirement | Priority | Acceptance evidence |
|---|---|---|
| `help`, `ls`, `cat`, `mkdir`, `rm`, `mount`, `umount`, `sysinfo`, `ps` or equivalent | P0/P1 | Built as user programs where appropriate; command behavior documented |
| System logs are inspectable from userland | P0 | Stable read-only interface and bounded log behavior |
| Memory and CPU info are truthful | P1 | Counters checked against kernel accounting and QEMU topology |
| Poweroff/reboot works on declared QEMU platform | P1 | ACPI/QEMU test; do not advertise unsupported behavior |
| Clean build and static checks | P0 | CI build, formatting and lint policy |
| QEMU boot + smoke tests | P0 | CI catches boot failure, shell failure, process failure and filesystem failure |
| Release artifact and image reproducibility | P0 | Checksums, build instructions, known limitations and reproducible image creation |
| Version metadata synchronized | P0 | README, roadmap, architecture, changelog and Git tag agree |

## 19. The v0.1 end-to-end demonstration

The release should demonstrate one complete, observable user journey rather than a large number of disconnected kernel features.

### Demonstration scenario

1. Build Serix from a clean checkout using documented dependencies.
2. Boot via Limine in QEMU using the declared BIOS/UEFI configuration.
3. Initialize configured CPUs and show detected memory, topology and devices.
4. Start Ring 3 init and required service processes.
5. Start Ring 3 `rsh` as the normal shell.
6. Run `sysinfo` and `ps` to inspect kernel/user services and CPU configuration.
7. List and inspect a mounted persistent volume.
8. Create a directory and file, write content, read it back, rename/delete where supported.
9. Launch an independent ELF utility with arguments and environment variables.
10. Pipe one command into another and redirect output to a file.
11. Run a restricted operation without authority and observe a denial plus audit record.
12. Run a deliberately faulting child and prove that the shell/kernel survive.
13. Reboot QEMU and verify that persisted data remains intact.
14. Execute the same steps through automated smoke tests and capture serial logs/artifacts.

### Acceptance table

| Scenario | Expected result | Must not happen |
|---|---|---|
| Boot with 1 CPU | Kernel and Ring 3 environment reach shell | Triple fault, hang or unexplained panic |
| Boot with multiple CPUs | Configured APs initialize and scheduler remains stable | AP race, run-queue corruption or timer storm |
| Execute external command | Child runs in Ring 3 and returns a status | App code executes in Ring 0 or shell wedges |
| Malformed executable | Loader rejects input with defined error | Kernel panic or corrupted address space |
| Invalid user pointer | Error or contained process fault | Unchecked kernel page fault |
| Unauthorized file/IPC operation | Access denied and auditable | Operation succeeds due missing authorization |
| Filesystem write/reboot/read | Data persists on declared image | Silent corruption or false success |
| User process crashes | Fault is contained | Kernel or unrelated services crash |
| Required service exits | Init applies declared restart/failure policy | Silent disappearance with no diagnostics |
| CI clean build | Artifact builds from clean checkout | Hidden local dependency or stale generated artifact |

## 20. Recommended implementation sequence

Do not split this into many parallel feature branches. Follow the dependency chain and insist on measurable exit criteria.

| Stage | Focus | Deliverables | Exit condition |
|---|---|---|---|
| **M0 — Freeze contracts** | Release scope and architecture contracts | Syscall table, process ABI, FD semantics, capability inheritance, filesystem support statement, status vocabulary | Every advertised v0.1 feature has a testable contract |
| **M1 — Kernel/runtime correctness** | Context switch, memory, teardown and SMP | FPU/SSE/XSAVE state, frame reclamation, stack safety, TLB correctness, task and address-space teardown | Repeated process/mapping/SMP stress tests pass |
| **M2 — Real userland** | Init, service lifecycle and shell | Ring 3 init, Ring 3 `rsh`, argv/envp, external ELF launch, wait/exit status, service health policy | User can run multiple independent commands and recover from child failure |
| **M3 — I/O and persistence** | VFS, descriptors, console and filesystem | Correct standard streams, handles, pipes, redirection, persistent FS test matrix | Full file/pipe/reboot journey passes |
| **M4 — Security closure** | Capabilities and LES | Exhaustive authorization, process authority, policy failure rules, audit inspection and negative tests | Unauthorized operations demonstrably fail |
| **M5 — Operations and diagnostics** | Tools and recovery | `ps`, `mem`, `sysinfo`, logs, mount/device inventory, recovery shell, reboot/poweroff | System is diagnosable without attaching a debugger for routine issues |
| **M6 — Release engineering** | CI, documentation and artifacts | QEMU boot CI, smoke/stress tests, image checksums, changelog, compatibility/support statement | Tagged release is reproducible and traceable |

### Detailed ordering inside M1

1. Audit context-switch assembly and confirm all architecturally required state is preserved or intentionally managed (including FPU/SSE/XSAVE state).
2. Verify kernel-stack and user-stack transitions for every task lifecycle path.
3. Implement physical-frame deallocation and account for every owner/reference.
4. Verify TLB invalidation/shootdown before reclaiming mappings across CPUs.
5. Stress blocking/wakeup paths, IPC queue boundaries, priority inheritance and timer preemption.
6. Add contained user-process fault handling and complete task teardown.

### Detailed ordering inside M2

1. Define `spawn(path, argv, envp, inherited_handles, capabilities)` semantics before expanding syscall surface.
2. Harden ELF parsing and reject malformed or unsupported segments safely.
3. Implement Ring 3 init as the owner of service startup and shutdown policy.
4. Port `rsh` to `#![no_std]` + `ulib`; keep kernel shell only for recovery/debug.
5. Implement PATH lookup and external ELF execution.
6. Verify wait/exit status, FD inheritance/duplication and close-on-exit semantics.
7. Build basic utilities as independent programs where possible (`echo`, `ls`, `cat`, `mkdir`, `rm`, `pwd`, `ps`, `sysinfo`).

## 21. Long-range roadmap toward a credible general-purpose OS

The versions below are directional milestone labels, not calendar promises. Each major subsystem should have its own design, implementation and validation plan. Do not turn the version numbers into an excuse to ship untested code.

| Milestone | Theme | Main outcomes | Strategic significance |
|---|---|---|---|
| **v0.1.0** | Foundational systems preview | Ring 3 init/shell, independent static ELF programs, process/FD contracts, persistence, capability tests, QEMU CI | Proves Serix is becoming an OS with a real userland, not just a kernel console. |
| **v0.2.x** | Kernel and service hardening | Complete VM reclamation, process lifecycle, stable IPC/ABI, service supervision, audit and security closure | Makes the base safe enough for more complex services. |
| **v0.3.x** | Driver/resource architecture | PCI resource manager, safe MMIO/IRQ/DMA delegation, IOMMU domains and first isolated driver/server | Turns the hybrid model into a demonstrable hardware architecture. |
| **v0.4.x** | Networking foundation | VirtIO-net, Ethernet path, IPv4/IPv6, ARP/NDP, ICMP, UDP/TCP, DHCP/DNS, sockets and network policy | Enables ordinary network-connected applications and headless administration. |
| **v0.5.x** | Application runtime | Dynamic ELF linker, shared libraries, TLS, thread model, richer process APIs, signals/events, terminal/PTY semantics and libc strategy | Moves from test binaries toward real applications. |
| **v0.6.x** | Hardware enablement | NVMe, xHCI, HID, ACPI power management, laptop power/thermal foundations, real-hardware matrix | Begins supporting machines beyond virtual devices. |
| **v0.7.x** | Graphics and interactive environment | Display driver model, input service, compositor/window server, graphics API, fonts, audio foundations | Makes Serix viable as a desktop OS rather than a terminal-only system. |
| **v0.8.x** | Distribution and application ecosystem | Installer, package manager, signed packages, update/rollback, SDK, native app ports, docs and developer workflow | Makes the OS installable, maintainable and useful to people other than its author. |
| **v0.9.x** | Compatibility and product hardening | POSIX/API coverage based on application demand, selected compatibility runtime experiments, security audit, soak tests, performance baselines | Reduces adoption friction and exposes real product gaps before stability commitments. |
| **v1.0.0** | First stable supported platform | Frozen ABI policy, declared hardware/workload matrix, installer/update/recovery contract, security process, complete release test suite | A stable general-purpose platform for a defined scope—not a claim of Linux parity. |
| **v1.x–v2.x** | Broaden real-world capability | More hardware, desktop/server applications, developer tools, virtualization/container foundations where justified, broader filesystem and networking support | Earns credibility in selected user segments through working workloads. |
| **v2.x+** | Competitive alternative in defined domains | Reliable daily-use desktop and/or server profiles, meaningful application availability, performance and security comparisons, sustained release/support process | Only here should Serix make evidence-backed claims of being a credible alternative for named workloads. |

### Long-term adoption strategy

| Workstream | Direction | Decision rule |
|---|---|---|
| Native application ecosystem | First-class Serix SDK, libraries, docs, packages and ports | Every ABI/runtime addition should enable a real class of apps. |
| POSIX/API compatibility | Implement high-value interfaces without letting Linux semantics dictate kernel internals | Prioritize based on target software and maintainability. |
| Linux binary compatibility | Treat as a major optional adoption project, not a checkbox | Start only when process, VM, signals, threading, ELF dynamic linking and libc foundations can support a credible subset. |
| Desktop | Build a complete user-facing environment with input, graphics, compositor and apps | Do not begin with visual polish; start with hardware/display/input contracts. |
| Server/headless | Reliable services, networking, storage, remote management, logs and upgrades | Soak tests and recoverability matter more than feature count. |
| Virtualization/containers | Explore after strong VM, isolation, networking and resource accounting | Do not promise Docker compatibility before namespaces/cgroups-equivalent mechanisms and security model exist. |
| Hardware support | Publish supported/untested/unsupported device lists | A driver is supported only after real hardware testing and failure handling. |
| Security and lifecycle | Threat model, secure boot/update, vulnerability reporting, rollback and support policy | Security claims require independent review and negative/fault-injection evidence. |

## 22. Decisions to make now

| Decision | Recommendation | Reason |
|---|---|---|
| Product identity | General-purpose operating system with a staged path to Linux-alternative credibility | Avoids shrinking Serix into a command-line niche. |
| Initial validation platform | x86-64 QEMU/OVMF + a narrowly declared VirtIO device set | Gives reproducible low-level development without defining the final hardware scope. |
| Architecture | Hybrid kernel; move components according to isolation, authority and measured performance | Neither “everything in Ring 0” nor “everything in Ring 3” is a design principle. |
| Native ABI | Serix-native, versioned, explicitly documented | Preserves design freedom and prevents accidental compatibility promises. |
| Compatibility strategy | POSIX and Linux compatibility are separate workstreams with explicit target applications | Compatibility should solve adoption problems, not become architecture by imitation. |
| Userland | Ring 3 init, shell, services and utilities | Establishes the user/kernel boundary and a native app platform. |
| Process model | Complete spawn/exec/wait/exit and resource teardown first; threads/signals next | Dependencies for services, libc and applications. |
| Filesystems | One fully tested persistent baseline; ext4 remains experimental until acceptance tests pass | Integrity matters more than a list of formats. |
| Driver architecture | Safe resource broker and IOMMU/DMA model before untrusted Ring 3 hardware drivers | Avoids turning “microkernel” into a false security claim. |
| Security defaults | Explicit operation-specific policy; privileged/resource-sensitive operations fail closed | Least privilege must survive service failure. |
| Networking | Userspace protocol services with explicit buffer ownership and a stable socket/resource API | Makes networking testable without swelling the kernel. |
| GUI | Long-term requirement, not a v0.1 distraction | A general-purpose desktop alternative needs a graphical environment. |
| Release policy | Evidence-based gates and a hardware/workload support matrix | Version numbers should communicate tested capability. |

## 23. What not to do before v0.1.0

These are sequencing constraints, not cancellations of the long-term product vision.

- Do not add NVMe, xHCI, Wi-Fi, audio and GPU drivers in parallel before resource ownership and testing are ready.
- Do not start a GUI/window manager before process, memory, input and display contracts are sufficiently stable.
- Do not chase full POSIX or Linux ABI coverage before the native process and runtime contracts work.
- Do not introduce another scheduler policy before the existing SMP scheduler passes stress and correctness tests.
- Do not claim IOMMU isolation because ACPI DMAR parsing works.
- Do not claim Ring 3 driver isolation because `ext4d` and `policyd` run in Ring 3.
- Do not claim ext4 crash safety until interrupted-write recovery and host-interoperability tests pass.
- Do not call a preallocated page cache integrated until filesystem I/O, mmap coherence and writeback use it correctly.
- Do not build a package manager before external commands, stable file semantics and reproducible system images exist.
- Do not spend time on branding or feature-count inflation while process isolation, resource reclamation and security enforcement remain incomplete.
- Do not let “v0.1 scope” become the permanent scope of Serix. Networking, hardware, graphics, packages, applications and compatibility remain visible roadmap pillars.

The immediate priority is to build a complete vertical slice and harden its invariants. The next priority is to expand that slice into real hardware and useful applications.

## 24. Definition of done for v0.1.0

- [ ] The release version is synchronized across README, architecture, roadmap, changelog and Git tag.
- [ ] A clean checkout builds using documented instructions.
- [ ] QEMU boots Serix with 1 and multi-CPU configurations to Ring 3 init and `rsh`.
- [ ] The normal shell is a userspace process; `kshell` remains available for recovery/debug.
- [ ] The shell launches independent static ELF programs from the filesystem.
- [ ] Arguments, environment, exit status and parent/child lifecycle are tested.
- [ ] File descriptors have unambiguous per-process ownership, inheritance and teardown.
- [ ] stdin/stdout/stderr, redirection and advertised pipe semantics work.
- [ ] One declared persistent filesystem passes create/read/update/delete/reboot and host-interoperability tests.
- [ ] Capability denial tests cover every exposed resource class and process-control path.
- [ ] Policy-service failure behavior is explicit and secure for privileged operations.
- [ ] FPU/SSE/extended-state handling, frame reclamation, address-space teardown and SMP context switching pass stress tests.
- [ ] User-process faults do not crash the kernel or unrelated processes.
- [ ] Basic process, memory, CPU, mount and kernel-log inspection is available from userland.
- [ ] Automated boot, syscall, process, filesystem and smoke tests run from CI.
- [ ] Release artifacts, checksums, build instructions, supported QEMU configuration and known limitations are published.

If a P0 item is missing, do not quietly redefine the milestone. Keep it in `v0.0.x` or label it an explicitly incomplete preview. A release number is not more important than the meaning attached to it.

# Appendix A — Historical MS-DOS concepts and modern equivalents

| Historical DOS concept | Original role | Modern equivalent | Serix relevance |
|---|---|---|---|
| `IO.SYS` | Hardware-dependent I/O and driver layer | HAL, firmware handoff, device services | Kernel HAL + future driver servers |
| `MSDOS.SYS` | DOS kernel services | Kernel core and system services | Kernel ABI, scheduler, memory, IPC, VFS dispatch |
| `COMMAND.COM` | Replaceable command interpreter | Ring 3 shell | `rsh` |
| INT 21h | Main DOS function API | Stable syscall ABI | Serix-native syscall interface |
| PSP | Per-program metadata block | Kernel process object + initial user stack | `TaskCB`, process metadata and startup ABI |
| MCB | Conventional-memory block metadata | VMAs, frame allocator and process memory accounting | Memory manager |
| `.COM` / MZ `.EXE` | DOS executable formats | Static/dynamic ELF64 | ELF loader |
| FCB | Legacy record-oriented file interface | File descriptor/handle | Do not reproduce FCB as the native API |
| `CON`, `NUL`, `AUX`, `PRN` | Logical character devices | `/dev` nodes or typed handles | Device namespace |
| `CONFIG.SYS` | Driver and kernel configuration | Declarative boot/service configuration | Init/config service |
| `AUTOEXEC.BAT` | Startup commands | Init service manifests and scripts | Ring 3 init |
| TSR | Resident background program | Supervised daemon/service | Init + IPC + scheduler |
| `LOADHIGH` / UMB | Place drivers in upper memory | Per-process VM and memory policy | Not relevant as an implementation pattern |
| XMS/EMS | Extended/expanded memory APIs | Virtual memory, shared memory and mappings | Memory manager |
| `SMARTDRV` | Disk cache | Unified page/block cache | VFS/page cache |
| `MSCDEX` | CD-ROM extension | Filesystem service + optical device driver | Future device/FS services |
| `SHARE` | File and record sharing/locking | VFS locks and explicit sharing modes | Later VFS feature |
| Network redirector | Route remote file operations | Network filesystem mounted into VFS | NetDaemon + VFS |
| DOSKEY | Command history and macros | Shell line editor/history/completion | `rsh` |
| `CHKDSK` / `SCANDISK` | Filesystem/disk diagnostics | Filesystem checker and recovery tools | Future `fsck` equivalent |

---

# Appendix B — Repository and technical references

## Serix repository references reviewed

- [Serix repository](https://github.com/gitcomit8/serix)
- [README implementation status](https://github.com/gitcomit8/serix/blob/dev/README.md)
- [Development roadmap](https://github.com/gitcomit8/serix/blob/dev/docs/ROADMAP.md)
- [Kernel architecture](https://github.com/gitcomit8/serix/blob/dev/docs/ARCHITECTURE.md)
- [v1.0 checklist](https://github.com/gitcomit8/serix/blob/dev/docs/CHECKLIST_v1.md)
- [Reviewed `dev` commit](https://github.com/gitcomit8/serix/commit/2550e02567d9db99413c52951604b01e791f7de7)

## Historical MS-DOS references

- [The MS-DOS Encyclopedia — Section I: Development of MS-DOS](https://msarchive.pcjs.org/mspl13/msdos/encyclopedia/section1/)
- [The MS-DOS Encyclopedia — Section II: Programming in the MS-DOS Environment](https://msarchive.pcjs.org/mspl13/msdos/encyclopedia/section2/)
- [The MS-DOS Encyclopedia — Section V: System Calls](https://msarchive.pcjs.org/mspl13/msdos/encyclopedia/section5/)
- [Microsoft MS-DOS 1.25 and 2.0 source code](https://github.com/microsoft/MS-DOS)
- [Writing MS-DOS Device Drivers, Second Edition](https://www.bitsavers.org/pdf/microsoft/msdos_5/Lai_-_Writing_MS-DOS_Device_Drivers_2ed_1992.pdf)

## Modern platform references

- [UEFI Forum specifications](https://uefi.org/specifications)
- [UEFI Forum: ACPI specifications and overview](https://uefi.org/acpi/specs)
- [PCI-SIG specifications](https://pcisig.com/specifications)
- [OASIS VirtIO specification](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html)
- [NVM Express specifications](https://nvmexpress.org/specifications/)
- [Linux kernel ext4 documentation](https://docs.kernel.org/filesystems/ext4/)
- [Limine boot protocol](https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md)

---

## Final recommendation

Serix's ambition is a credible general-purpose operating system and, eventually, a serious alternative to Linux. The modernized DOS study is useful because it reminds us that an OS is a complete operating environment: startup, services, files, devices, shell, diagnostics, applications and recovery. It is not the product identity and it is not the architectural ceiling.

Do not optimize for the number of kernel subsystems implemented. Optimize for complete, tested user outcomes, then expand hardware, networking, graphics, application compatibility and ecosystem breadth without compromising the hybrid-kernel security model.

**v0.1.0 should prove that Serix has a real, isolated userland and dependable kernel contracts. Every subsequent milestone should move it closer to being installable, useful, secure and reliable on real machines for real workloads.**
