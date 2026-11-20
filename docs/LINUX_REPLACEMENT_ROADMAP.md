# Serix: Long-Term Linux Alternative Roadmap

> **Status snapshot:** 2026-11-13  
> **Repository branch audited:** `dev`  
> **Roadmap baseline:** `docs/ROADMAP.md` (v0.0.9) and `docs/ARCHITECTURE.md`  
> **Target architecture:** x86_64 (AMD64)  
> **Document type:** Strategic product and engineering roadmap — not a release promise.

---

## 1. Vision and product definition

Serix is intended to become a **credible, general-purpose operating system and, over the long term, a viable Linux alternative**. It is not being built merely as an educational kernel, a DOS-like environment, a QEMU demonstration, or a small hobby OS.

The long-term objective is an operating system that can be installed on real computers, run a broad range of native applications and developer workloads, support common hardware, provide networking and a usable graphical environment, and be maintained reliably over time.

The goal is not to reproduce Linux internally. Serix should retain its own kernel architecture, native system-call ABI, and security model while progressively providing the compatibility layers and ecosystem needed to run real-world software.

### Product principles

1. **General-purpose first:** design core interfaces for workstations and servers, not only for demos.
2. **Native architecture, compatibility where valuable:** preserve a Serix-native ABI; build POSIX compatibility and, eventually, substantial Linux userspace compatibility.
3. **Security by construction:** capabilities, least privilege, and isolated services should be real enforceable properties, not marketing language.
4. **Hardware is part of the OS:** a kernel that only works in a narrow emulator configuration is not a general-purpose operating system.
5. **End-to-end completeness beats feature checkboxes:** a driver, filesystem, or syscall is not complete until its error paths, concurrency, recovery, and integration are tested.
6. **Evidence over claims:** performance, security, compatibility, and reliability claims must be backed by repeatable tests and published support matrices.
7. **Maintainability is a feature:** stable interfaces, diagnostics, documentation, CI, release engineering, and contributor workflows are essential.

### Intended initial audience and deployment target

The recommended first target is **developer workstations and general-purpose x86-64 computers**, beginning with reproducible QEMU/KVM environments and a deliberately defined set of modern Intel/AMD platforms.

Initial priorities:
- UEFI boot and a dependable QEMU/KVM reference platform.
- A complete CLI-first user environment.
- Reliable storage, process management, and networking.
- A working C and Rust development environment.
- A narrow but real physical-hardware support matrix before claiming broad laptop or desktop support.

Servers, laptops, broader desktop support, and wider hardware coverage follow as the platform matures.

---

## 2. Current implementation baseline

This section records what the `dev` branch documentation says exists today. A checked box means the repository's current roadmap marks the capability implemented; it does **not** imply production-grade maturity. Partial or experimental capabilities are explicitly identified.

### 2.1 What Serix already has

- [x] x86_64 `#![no_std]` Rust kernel and Limine v10.x boot integration.
- [x] BIOS and UEFI boot paths documented for the current Limine setup.
- [x] Higher-half kernel and HHDM memory mapping.
- [x] CPU exception handling, IDT, LAPIC, I/O APIC, legacy PIC disable, and LAPIC timer preemption.
- [x] ACPI RSDP/root/SDT validation and MADT, FADT, and DMAR table decoding for discovery.
- [x] xAPIC/x2APIC access selection and fixed IPI send support.
- [x] Limine MP-based AP bootstrap, per-CPU data through GS_BASE, per-CPU run queues, and SMP operation.
- [x] Preemptive per-CPU WFQ scheduling and priority inheritance.
- [x] Task control blocks, asynchronous task infrastructure, context switching, and kernel stacks.
- [x] Serix-native `SYSCALL`/`SYSRET` entry and a 29-syscall interface documented in the current roadmap.
- [x] Ring 3 ELF loading for static `PT_LOAD` binaries and `PT_GNU_STACK`.
- [x] Per-task capability spaces, capability validation, delegation/revocation primitives, generation checks, and authorization helpers.
- [x] LES (Least-Elevated Security) policy bridge, Ring 3 `policyd`, TOML policy parsing, hot reload, and an audit ring.
- [x] VFS, mount table, path resolution, file descriptors, and standard descriptors.
- [x] FAT32 read/write support, including long file names and Linux image interoperability.
- [x] Ring 3 `ext4d` daemon with ext4 superblock, group descriptor, inode, extent, directory, read/write, and JBD2/HTree implementation work.
- [x] Page-cache data structures, LRU/dirty tracking helpers, demand paging, file-backed mappings, and `mmap`/CoW support.
- [x] VirtIO-blk driver and byte-oriented VFS block-device integration.
- [x] PCI enumeration, safe BAR decoding, device class identification, and bounded MSI-X metadata discovery.
- [x] PS/2 keyboard input and framebuffer/serial console output.
- [x] Kernel-resident `kshell` with filesystem, logging, policy, mount, and system commands.
- [x] QEMU boot/build workflow documented in the repository.

### 2.2 Current work is not yet equivalent to a usable Linux replacement

The following limitations are especially important. They should shape engineering priorities more than the number of implemented subsystems.

- [ ] **Physical memory reclamation is incomplete:** the current `StaticBootFrameAllocator` has no general frame deallocation path. This blocks long-running, memory-intensive workloads and safe VM lifecycle management.
- [ ] **VM correctness and reclamation are incomplete:** TLB shootdown primitives exist, but wiring them into VMA reclamation and related paths remains unfinished.
- [ ] **Page cache is not integrated end-to-end:** helpers exist, but the FAT32/ext4 paths do not yet use the unified cache and writeback machinery.
- [ ] **FPU/SSE/AVX task-state management is incomplete:** robust extended processor-state save/restore is required before general userspace workloads can be trusted.
- [ ] **Process model is incomplete:** `fork`/`clone`, `execve`, dynamic linking, signals, and full process/thread semantics are not complete.
- [ ] **Userspace is immature:** `kshell` is Ring 0; the planned Ring 3 `rsh` port, init/service management, and independent userland are not yet complete.
- [ ] **Storage reliability is not proven:** ext4 journaling and recovery code exist, but interrupted-write replay, Linux `e2fsck` interoperability, malformed-metadata rejection, and other acceptance tests remain.
- [ ] **Hardware support is narrow:** VirtIO-blk and PS/2 exist; NVMe, xHCI/USB, HID, Wi-Fi, Bluetooth, audio, GPU acceleration, and broad laptop support are not implemented.
- [ ] **DMA isolation is absent:** DMAR discovery exists, but IOMMU page tables, per-device DMA domains, and interrupt remapping are not implemented.
- [ ] **Networking is not implemented:** no completed network driver, TCP/IP stack, socket API, DHCP, DNS, firewall, or ordinary network services.
- [ ] **Power management is incomplete:** ACPI discovery exists, but robust ACPI shutdown/reboot, CPU idle states, performance states, and thermal management remain.
- [ ] **No mature application ecosystem:** no stable POSIX layer, libc/dynamic linker stack, package manager, broad native applications, or Linux userspace compatibility.
- [ ] **No complete graphical desktop:** framebuffer console is not a desktop stack; display, GPU drivers, compositor, audio, and desktop services remain future work.
- [ ] **Reliability infrastructure is immature:** the current developer notes say there is no automated test suite; systematic CI, stress testing, fuzzing, fault injection, and hardware qualification are required.

### 2.3 Repository documentation consistency

The current `docs/ROADMAP.md` identifies the branch as v0.0.9 and was updated 2026-11-13. Other documentation (notably `CLAUDE.md`) contains older status/version statements. Treat the current roadmap and code/tests as the baseline for this tracker, and reconcile stale documentation as part of release hygiene.

---

## 3. The 15 pillars of a credible general-purpose OS

The following are the long-term capability areas Serix must address. They are intentionally broader than the current kernel roadmap.

### 3.1 Production-grade kernel

- [ ] Complete physical frame allocation and reclamation, including fragmentation handling and allocator invariants.
- [ ] Robust virtual memory: address-space lifecycle, page faults, page sharing, CoW, protection, swapping strategy if required, and safe unmapping.
- [ ] Correct SMP synchronization, preemption, interrupt handling, timers, and cross-CPU coordination.
- [ ] Complete FPU/SSE/AVX state management and architecture-specific context switching.
- [ ] Stable process and thread lifecycle, including creation, termination, waiting, and resource cleanup.
- [ ] Robust kernel stacks, guard pages, stack overflow detection, and kernel memory protection.
- [ ] NUMA awareness as hardware support expands.
- [ ] Panic diagnostics, crash dumps, tracing, and post-mortem debugging.
- [ ] Documented invariants and stress tests for all critical kernel paths.

### 3.2 Comprehensive hardware support

- [ ] Reliable UEFI boot, ACPI interpretation, PCIe enumeration, MSI/MSI-X, and interrupt routing.
- [ ] NVMe, SATA/AHCI, and USB mass-storage support.
- [ ] xHCI, USB device lifecycle, HID keyboard/mouse, and hot-plug.
- [ ] Ethernet and Wi-Fi support, followed by Bluetooth.
- [ ] Audio device support and a user-facing audio service.
- [ ] Display stack and GPU support for relevant Intel, AMD, and NVIDIA hardware.
- [ ] Laptop essentials: battery, thermal sensors, suspend/resume, brightness, keyboard hotkeys, and power management.
- [ ] IOMMU-backed DMA isolation and interrupt remapping.
- [ ] A published hardware compatibility list with tested device/firmware combinations.

### 3.3 Storage and filesystems

- [ ] A stable VFS contract with mount namespaces, file descriptors, permissions, locking, and consistent error semantics.
- [ ] Production-quality FAT32 and ext4 support, including crash consistency and recovery.
- [ ] Additional interoperability formats as justified: exFAT and NTFS (read/write support should be scoped realistically).
- [ ] Integrated page cache, writeback, readahead, cache coherency, and memory-mapped file correctness.
- [ ] Symbolic links, hard links, timestamps, ownership, ACLs, and extended attributes.
- [ ] Filesystem checking and repair tooling.
- [ ] Encryption and secure key handling.
- [ ] Volume management, RAID, snapshots, and advanced storage features when the core is reliable.

### 3.4 Networking

- [ ] Ethernet and Wi-Fi drivers.
- [ ] IPv4 and IPv6, ARP/NDP, ICMP, TCP, and UDP.
- [ ] DNS, DHCP, routing, loopback, and socket APIs.
- [ ] Firewalling and network policy integration.
- [ ] TLS integration and certificate/key management.
- [ ] Unix-domain sockets and local IPC interoperability.
- [ ] VPN support and standard network tooling.
- [ ] Common services/protocols such as SSH, HTTP, NFS, and SMB.
- [ ] Network isolation and container networking foundations.

### 3.5 Security architecture

- [ ] Complete process and address-space isolation.
- [ ] Capability enforcement on every privileged resource path, including process control, devices, DMA, and IPC.
- [ ] Fail-closed policy behavior for security-sensitive decisions.
- [ ] POSIX UID/GID/mode/ACL compatibility where needed.
- [ ] ASLR, W^X, stack protection, guarded mappings, and secure entropy.
- [ ] Secure boot and verified/signed update chain.
- [ ] IOMMU isolation, interrupt remapping, and safe device access.
- [ ] Sandboxing and service-level privilege separation.
- [ ] Auditing, key management, security event reporting, and vulnerability response.
- [ ] Independent security review and adversarial testing.

### 3.6 Complete userspace ABI

- [ ] A documented, versioned, stable Serix-native syscall ABI.
- [ ] Complete process/thread lifecycle and resource semantics.
- [ ] Signals or a carefully specified equivalent event model.
- [ ] Pipes, sockets, shared memory, synchronization primitives, and descriptor passing.
- [ ] Reliable `mmap`, file I/O, filesystem, and device interfaces.
- [ ] TLS/thread-local storage and dynamic loader support.
- [ ] POSIX compatibility layer with a clearly stated conformance target.
- [ ] ABI compatibility tests across releases.

### 3.7 C and Rust runtime ecosystem

- [ ] A usable libc and POSIX runtime.
- [ ] pthreads or equivalent thread runtime.
- [ ] Dynamic linker/loader and shared-library support.
- [ ] LLVM/Clang toolchain support.
- [ ] Native Rust target and standard-library support where feasible.
- [ ] GCC support as a later ecosystem milestone.
- [ ] Build tools, linker, debugger, profiler, and system headers.
- [ ] Cross-compilation SDK and reproducible build environment.

### 3.8 Init and service management

- [ ] A Ring 3 init process (PID 1 equivalent).
- [ ] Dependency-aware service startup and shutdown.
- [ ] Service supervision, restart policies, and health checks.
- [ ] Service-specific identities, capabilities, and sandboxing.
- [ ] User/session management and login.
- [ ] Device discovery and service activation.
- [ ] Structured system logging and log rotation.
- [ ] Reliable shutdown, reboot, recovery, and boot targets.

### 3.9 Graphics and desktop

- [ ] Display and GPU driver architecture.
- [ ] A stable display server/compositor (Wayland-compatible protocol or a deliberate alternative).
- [ ] Input services for keyboard, mouse, touch, and hot-plug.
- [ ] Audio server and device routing.
- [ ] Fonts, clipboard, notifications, accessibility, and window management.
- [ ] Multi-monitor, scaling, HiDPI, and graphics acceleration.
- [ ] Desktop environment and core user-facing settings/tools.

### 3.10 Application compatibility

- [ ] Native Serix application format and runtime conventions.
- [ ] POSIX application compatibility.
- [ ] Linux userspace compatibility as a strategic long-term track (not an assumption that Linux binaries will run automatically).
- [ ] Virtualization/emulation for workloads that cannot be supported natively.
- [ ] Evaluate Wine only after the underlying ABI, graphics, and filesystem foundations exist.
- [ ] Support key developer and productivity applications: browser, terminal, IDE, editor, office tools, Java, Python, and Node.js.
- [ ] Compatibility test suite and published application matrix.

### 3.11 Package management and updates

- [ ] Package format and metadata model.
- [ ] Dependency resolver and version constraints.
- [ ] Signed repositories and package verification.
- [ ] Atomic installation and rollback.
- [ ] System updates, recovery from failed updates, and security patch delivery.
- [ ] Third-party repositories and developer publishing workflow.
- [ ] Build infrastructure and reproducible packages.

### 3.12 Developer ecosystem

- [ ] Stable SDK, headers, libraries, and API documentation.
- [ ] Compiler, linker, debugger, profiler, tracing, and crash-dump tooling.
- [ ] Emulator/VM workflows and hardware test harnesses.
- [ ] System-call and driver development guides.
- [ ] CI templates, sample applications, and porting documentation.
- [ ] Clear compatibility/versioning policy for developers.

### 3.13 QA, reliability, and performance

- [ ] Unit and integration tests for kernel and userspace components.
- [ ] Filesystem stress, crash consistency, and recovery tests.
- [ ] SMP race, deadlock, and memory-ordering tests.
- [ ] Syscall, protocol, filesystem, and driver fuzzing.
- [ ] Driver fault injection and device reset/error-path tests.
- [ ] Long-running stability tests and memory-leak detection.
- [ ] Repeatable performance benchmarks against Linux on identical hardware/configuration.
- [ ] Hardware compatibility and regression matrix.
- [ ] Reproducible builds and release artifacts.

### 3.14 Installer, recovery, and deployment

- [ ] Bootable installation media and UEFI installation.
- [ ] Disk/partition management and filesystem setup.
- [ ] Initial user and administrator creation.
- [ ] Network configuration during installation.
- [ ] Recovery/rescue environment and filesystem repair.
- [ ] Upgrade and rollback workflow.
- [ ] Secure update mechanism.
- [ ] Unattended/server deployment path.
- [ ] Documented supported install configurations.

### 3.15 Governance and long-term maintenance

- [ ] Contribution guidelines, code review, and ownership conventions.
- [ ] Security disclosure policy and response process.
- [ ] Release engineering and supported release channels.
- [ ] LTS policy and security maintenance commitments.
- [ ] Stable ABI/API and driver compatibility guarantees.
- [ ] Technical decision records and subsystem ownership.
- [ ] Public issue triage and contributor onboarding.
- [ ] Community, application developer, and hardware vendor relationships.

---

## 4. Priority order: what to build next

The ordering below is deliberate. It prioritizes prerequisites and end-to-end usability over impressive but disconnected features.

### P0 — Make the kernel safe and dependable

**Do these before expanding the feature surface.**

1. **Memory lifecycle:** replace the boot-only frame allocation model with a reclaiming allocator; define ownership and lifetime rules; test allocation/free under pressure.
2. **VM correctness:** complete address-space teardown, page reclamation, TLB shootdown integration, page-table synchronization, and CoW correctness.
3. **FPU/extended state:** implement correct XSAVE/XRSTOR or an equivalent complete strategy; test task migration across CPUs.
4. **SMP correctness:** finish TSS.RSP0 handling, AP lifecycle gaps, scheduler-kick wiring, TLB shootdown integration, and concurrency stress tests.
5. **Process lifecycle:** specify and implement robust spawn/exit/wait/resource cleanup; then add thread semantics and `clone`/equivalent.
6. **IPC and capability audit:** complete authorization coverage, validate revocation and delegation under adversarial conditions, and define fail-closed LES semantics.
7. **Storage correctness:** add ext4 crash-replay, `e2fsck` interoperability, malformed metadata, and filesystem stress acceptance tests; wire page cache and writeback.
8. **Diagnostics and test foundation:** automated build/lint/format checks, deterministic QEMU boot tests, panic diagnostics, and regression tests.

**Exit condition:** repeated SMP QEMU runs can execute independent Ring 3 processes, reclaim memory, handle faults and process exit safely, and pass storage/capability regression tests without leaks or silent corruption.

### P1 — Deliver a genuinely usable CLI operating system

1. Port `rsh` to Ring 3 and complete its line editor, filesystem builtins, PATH lookup, and external command execution.
2. Implement `execve` and a coherent process/thread API; add signals or a specified alternative.
3. Add a Ring 3 init/service manager with supervision, logging, and clean shutdown/reboot.
4. Complete `/proc` observability: memory, CPU, uptime, process state, maps, and system tracing.
5. Finish NVMe and xHCI/USB foundations; implement HID input and reliable device lifecycle.
6. Implement ACPI power-off/reboot and baseline CPU idle/power management.
7. Build the driver-server framework: userspace MMIO mapping, DMA buffer ownership, interrupt delivery, device capabilities, and fault containment.
8. Publish a reproducible VM image and a narrow physical-hardware reference configuration.

**Exit condition:** boot to a Ring 3 login/shell, execute multiple independent programs, manage files and services, use USB input and NVMe storage on the reference hardware, and shut down/reboot cleanly.

### P2 — Networking and a native development environment

1. Implement VirtIO-net with robust queues, DMA lifecycle, and error recovery.
2. Integrate a `no_std` TCP/IP stack or equivalent userspace networking library.
3. Implement socket APIs, loopback, DHCP, DNS, IPv4/IPv6, TCP/UDP, and network configuration tools.
4. Add firewall and network policy integration.
5. Deliver libc and a useful POSIX subset.
6. Deliver dynamic linking, TLS, shared libraries, and a usable C/Rust toolchain.
7. Add package management, signed repositories, atomic updates, and rollback.

**Exit condition:** developers can build, install, run, debug, and update real applications on Serix; the OS can connect to ordinary networks and provide standard network services.

### P3 — Graphics, audio, and everyday hardware

1. Establish a stable graphics/display driver interface and compositor architecture.
2. Implement a usable graphics stack, input services, and multi-monitor support.
3. Add audio drivers and an audio service.
4. Expand Wi-Fi, Bluetooth, laptop power, thermal, suspend/resume, and GPU support.
5. Build a desktop environment and core desktop applications.
6. Publish hardware support tiers and test each release against them.

**Exit condition:** Serix is usable as a daily-driver workstation on a defined, growing set of real computers—not merely a command-line kernel demo.

### P4 — Linux application compatibility

1. Stabilize Serix's own ABI and POSIX compatibility surface.
2. Implement the Linux ABI compatibility layer incrementally, prioritizing high-value syscalls and semantics.
3. Support Linux ELF loading, dynamic linking, signals, futexes, sockets, procfs expectations, and other required interfaces.
4. Run a compatibility test suite and publish pass/fail results.
5. Add containers/namespaces and isolation primitives where the kernel supports them.
6. Evaluate Wine and broader application compatibility only after graphics, networking, and ABI foundations are mature.

**Exit condition:** a meaningful, documented set of Linux applications runs reliably, with clear boundaries around unsupported features.

### P5 — Production maturity and adoption

1. Independent security review and vulnerability disclosure/response process.
2. Long-running reliability, filesystem, SMP, and hardware qualification.
3. Secure boot, signed updates, recovery, and rollback.
4. LTS releases and compatibility guarantees.
5. Installer, migration, deployment, and fleet management.
6. Performance parity or compelling advantages in defined workloads.
7. Sustainable contributor, package, application, and hardware ecosystems.

**Exit condition:** users and organizations can deploy Serix with understood support boundaries, predictable maintenance, and credible operational guarantees.

---

## 5. Dependency-aware roadmap and maturity gates

These stages are capability gates, not calendar promises.

| Stage | Goal | Required outcome |
|---|---|---|
| **Stage 1 — Functional OS** | Dependable kernel and independent userspace | Stable memory/VM lifecycle, SMP, process/thread model, IPC/capabilities, VFS/storage, Ring 3 shell and init |
| **Stage 2 — Independent general-purpose OS** | Useful CLI OS on real hardware | Networking, NVMe/USB, device services, users, packages, C/Rust toolchain, service manager |
| **Stage 3 — Linux application alternative** | Run meaningful third-party software | POSIX, dynamic linking, Linux userspace compatibility, containers, graphics/desktop, broad hardware |
| **Stage 4 — Production OS** | Reliable deployment and maintenance | Security audits, validated updates/recovery, LTS, hardware qualification, stable interfaces |
| **Stage 5 — Credible Linux competitor** | Sustainable adoption | Competitive performance, application ecosystem, hardware/vendor support, contributors, real users |

A stage is not complete because a feature exists in source code. It is complete only when the capability is integrated, tested, documented, and usable through the normal system workflow.

---

## 6. Suggested release direction

These are illustrative maturity labels, not commitments to dates or exact feature counts. The existing repository phase plan remains the implementation-level tracker.

| Release target | Intended milestone |
|---|---|
| **v0.1.0** | Reliable independently usable OS foundation |
| **v0.2.x** | Stable process model, filesystem, and userspace |
| **v0.3.x** | Networking and meaningful hardware enablement |
| **v0.4.x** | Native developer environment and package management |
| **v0.5.x** | Graphics/audio/desktop foundations |
| **v0.6.x** | Substantial POSIX compatibility |
| **v0.7.x** | Linux application compatibility experiments |
| **v0.8.x** | Broad hardware support and real-world testing |
| **v0.9.x** | Release-candidate hardening, security, reliability, and upgrade validation |
| **v1.0.0** | Stable general-purpose OS with a defined hardware and application support matrix |

Do not let version numbers drive engineering decisions. A smaller release with clear acceptance criteria is better than a version bump that overstates maturity.

---

## 7. Architecture strategy and differentiators

Serix should not become a Linux clone. It should keep a coherent native design and add compatibility intentionally.

### Preserve a Serix-native core

- Keep the Serix-native syscall ABI and evolve it with explicit versioning.
- Do not copy Linux syscall numbers or Linux kernel internals merely to appear compatible.
- Build a POSIX layer as a compatibility boundary.
- Treat Linux ABI compatibility as a separate, measurable subsystem with its own test suite.

### Potential differentiators to prove

These are strategic hypotheses. They become real advantages only when backed by implementation evidence and benchmarks.

- **Rust implementation:** memory safety benefits in appropriate kernel and service code, while still treating unsafe code and hardware boundaries rigorously.
- **Capability-based security:** fine-grained authority and revocation rather than relying only on ambient privilege.
- **Isolated drivers and services:** move suitable drivers, filesystems, and protocol stacks into Ring 3 with constrained device, DMA, and interrupt access.
- **Heterogeneous CPU scheduling:** account for P-cores/E-cores and eventually NUMA topology with measured workload-aware policies.
- **Modular architecture:** allow services and drivers to fail/restart without taking down the kernel wherever the design permits.
- **Predictable resource management:** measurable scheduling, IPC, memory, and I/O behavior.

### Non-negotiable architecture rules

1. Do not move a driver to Ring 3 until the kernel has safe MMIO mapping, DMA ownership, interrupt delegation, IOMMU isolation where required, and revocation semantics.
2. Do not call a filesystem complete until crash consistency, corruption handling, interoperability, and stress criteria pass.
3. Do not call SMP complete solely because multiple CPUs boot; test races, migration, TLB coherence, interrupts, and failure paths.
4. Do not claim Linux compatibility until real applications and compatibility tests pass.
5. Do not optimize exotic scheduler policies ahead of memory safety, process correctness, and end-to-end functionality.

---

## 8. End-to-end acceptance criteria

Use these as release gates across subsystem boundaries.

### Kernel and memory
- [ ] Allocate/free/reuse frames under sustained pressure without leaks or corruption.
- [ ] Create/destroy address spaces and VMAs on multiple CPUs.
- [ ] Validate page faults, CoW, unmapping, and TLB shootdowns under concurrency.
- [ ] Preserve FPU/SSE/AVX state across preemption and migration.
- [ ] Run long-duration SMP stress tests without deadlock, corruption, or unexplained hangs.

### Processes and userspace
- [ ] Run multiple independent Ring 3 applications concurrently.
- [ ] Validate spawn, exec, wait, exit, resource cleanup, and process failure behavior.
- [ ] Support a documented thread model and TLS.
- [ ] Run a Ring 3 init and shell without relying on a kernel-resident shell for ordinary operation.

### Storage
- [ ] Read/write and mount supported filesystems from Serix.
- [ ] Mount Serix-created images on Linux and Linux-created images on Serix where the format promises interoperability.
- [ ] Interrupt writes at controlled points and validate recovery.
- [ ] Pass `e2fsck` and malformed-image tests for ext4 support claims.
- [ ] Exercise concurrent I/O, cache coherency, writeback, and device errors.

### Hardware and drivers
- [ ] Boot on the published reference hardware without undocumented manual workarounds.
- [ ] Exercise NVMe and USB device attach/detach and error recovery.
- [ ] Validate DMA isolation and interrupt delivery for isolated drivers.
- [ ] Verify power-off, reboot, and supported sleep/resume paths.

### Networking
- [ ] Obtain network configuration through DHCP.
- [ ] Resolve DNS and communicate over IPv4/IPv6.
- [ ] Validate TCP/UDP sockets, loopback, routing, and firewall policy.
- [ ] Stress network queues and recover from link/device failures.

### Security
- [ ] Test capability checks for every syscall and resource class.
- [ ] Test revocation, delegation, stale handles, confused-deputy cases, and race conditions.
- [ ] Verify fail-closed behavior for security-critical policy decisions.
- [ ] Fuzz syscall, IPC, filesystem, and driver-facing interfaces.
- [ ] Document the threat model and known limitations.

### Release engineering
- [ ] Automated build, lint, format, and QEMU boot tests.
- [ ] Reproducible release artifacts and checksums.
- [ ] Regression suite for supported hardware and applications.
- [ ] Installer, recovery, upgrade, and rollback tests.
- [ ] Published release notes, support matrix, and known issues.

---

## 9. Engineering allocation and execution discipline

As a planning heuristic—not a measured law—avoid spending all engineering effort inside the kernel. A mature OS is a kernel plus the software and operational ecosystem around it.

A reasonable long-term allocation to revisit as the project grows:

- **30–40%:** kernel, memory, scheduling, IPC, security, and architecture.
- **20–25%:** drivers, hardware enablement, and power management.
- **15–20%:** userspace, ABI, runtimes, and core system services.
- **10–15%:** filesystems, networking, and package infrastructure (adjust as these become critical path).
- **10–15%:** testing, tooling, documentation, release engineering, and community.

The percentages are intentionally approximate and overlap by subsystem. The point is to reserve substantial capacity for everything that turns a kernel into an operating system.

### Execution rules

- Work from dependency order, not novelty.
- Prefer finishing a vertical slice over starting five disconnected subsystems.
- Every major feature needs an acceptance test and a documented failure model.
- Keep a clear distinction between implemented, integrated, tested, and production-ready.
- Update this tracker when implementation status changes; link major items to issues/PRs.
- Reassess priorities after each maturity gate, not every time a new technology looks exciting.

---

## 10. Immediate recommended backlog

This is the practical next-work sequence based on the current `dev` roadmap snapshot.

### First: correctness blockers

- [ ] Implement physical frame deallocation/reuse and allocator accounting.
- [ ] Complete TLB shootdown integration with VMA/page-table reclamation.
- [ ] Implement complete FPU/extended processor state switching.
- [ ] Finish SMP scheduler-kick and per-task kernel-stack/TSS handling.
- [ ] Add process/address-space teardown and resource lifetime tests.
- [ ] Complete ext4 journal replay, `e2fsck`, malformed metadata, and stress acceptance tests.
- [ ] Integrate page cache into filesystem reads/writes and implement coherent writeback.
- [ ] Complete syscall authorization review, including process-control paths and adversarial tests.
- [ ] Define and enforce fail-closed LES behavior for security-sensitive policy decisions.

### Second: userspace that proves the kernel

- [ ] Complete Ring 3 `rsh` port and filesystem builtins.
- [ ] Implement `execve` and external command execution.
- [ ] Specify and implement thread/process semantics, then signals or equivalent.
- [ ] Build Ring 3 init/service supervision and clean shutdown/reboot.
- [ ] Complete useful `/proc` observability.
- [ ] Create a repeatable end-to-end MVP boot test.

### Third: real machine enablement

- [ ] Implement NVMe driver end-to-end, including queues, DMA, interrupts, timeout/error handling, and stress tests.
- [ ] Implement xHCI and HID input.
- [ ] Add IOMMU DMA domains and interrupt remapping before trusting general userspace drivers with device DMA.
- [ ] Implement ACPI S5/reset and baseline power/thermal management.
- [ ] Define the first physical-hardware support matrix.

### Fourth: independent general-purpose capability

- [ ] Establish userspace driver/service framework.
- [ ] Implement VirtIO-net and userspace TCP/IP/socket stack.
- [ ] Deliver libc/POSIX foundation and dynamic linker.
- [ ] Deliver package manager, signed repositories, and atomic update/rollback.
- [ ] Build C/Rust developer SDK and debugging tools.

Graphics, desktop polish, Wine, broad Linux application compatibility, and exotic scheduler optimization should follow—not precede—the above foundations.

---

## 11. What “Linux replacement” actually means

A Linux replacement is not achieved by implementing a large number of kernel features. It means a person can install Serix on supported hardware, use it for ordinary computing, install and update software, connect to networks, develop and run applications, recover from failures, and trust the system to keep working.

The target is therefore not “Linux feature parity” in the abstract. It is a sequence of increasingly demanding, demonstrable outcomes:

1. **Boot and survive:** reliable kernel, memory, process, and storage behavior.
2. **Operate independently:** Ring 3 userland, services, devices, and a real shell.
3. **Connect and build:** networking, toolchain, packages, and developer workflows.
4. **Work daily:** graphics, audio, laptop support, and useful applications.
5. **Adopt safely:** compatibility, security, updates, recovery, LTS, and support.
6. **Compete credibly:** performance, reliability, ecosystem, hardware coverage, and users.

The central strategic rule is simple: **design every subsystem for the eventual general-purpose OS, but only claim the capability once its complete user-visible path works and is tested.**

---

## 12. Status conventions

Use these conventions when maintaining this document:

- `[x]` Implemented and confirmed by the current repository tracker.
- `[~]` Partially implemented or integrated, but acceptance criteria remain.
- `[ ]` Not implemented or not yet validated.
- **Planned** does not mean committed to a release.
- A status change should cite the implementation, test, issue, or PR that justifies it.

This document is a strategic north star and cross-subsystem tracker. For granular implementation tasks, dependencies, and current phase details, continue maintaining [ROADMAP.md](ROADMAP.md) and [CHECKLIST_v1.md](CHECKLIST_v1.md). Keep the two layers synchronized: this file answers *what kind of OS Serix must become and in what order*; the implementation roadmap answers *which concrete tasks are being executed now*.
