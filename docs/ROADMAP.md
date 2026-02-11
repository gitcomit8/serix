# Serix Development Roadmap

:Last Updated: 2026-02-11
:Target Architecture: x86_64
:Current Release: v0.0.5

This document tracks the development roadmap for the Serix microkernel.
All dates are estimates and subject to change based on contributor
availability and technical challenges.

## Current Status (v0.0.5)

Released: 2026-02-11

The kernel has completed Phase 1 and Phase 2, with partial Phase 3
completion. Current capabilities:

- Boots via Limine v10.x on BIOS and UEFI
- APIC interrupt controller fully operational
- IDT with exception and hardware interrupt handlers
- Physical memory management with boot-time frame allocator
- Virtual memory with 4-level paging
- 1 MB kernel heap with linked_list_allocator
- LAPIC timer at ~625 Hz (vector 49)
- PS/2 keyboard driver (vector 33)
- Framebuffer graphics with text console
- VFS with ramdisk support (RamFile INode implementation)
- ELF loader for userspace binaries
- Basic syscalls: SYS_WRITE (1), SYS_READ (0), SYS_EXIT (60), SYS_YIELD (24)
- Async task executor with cooperative scheduling
- Capability system infrastructure (CapabilityStore)
- Serial console debug output (COM1, 115200 baud)

## Phase 1: Core Foundation (COMPLETED)

Status: 100% complete

Bootloader Integration

```~~~~~~~~~~~~~~~~~~~
 * [✓] Limine v10.x bootloader integration
 * [✓] Memory map parsing from Limine responses
 * [✓] Framebuffer initialization
 * [✓] HHDM (Higher Half Direct Map) offset handling

Memory Management
```~~~~~~~~~~~~~~
 * [✓] Page table initialization using bootloader CR3
 * [✓] Physical frame allocator (StaticBootFrameAllocator)
 * [✓] Heap allocator integration (1 MB at 0xFFFF_8000_4444_0000)
 * [✓] OffsetPageTable wrapper for page manipulation

Hardware Abstraction Layer
```~~~~~~~~~~~~~~~~~~~~~~~~

- [✓] CPU exception handlers (divide-by-zero, page fault, double fault)
- [✓] APIC setup (Local APIC + I/O APIC)
- [✓] Legacy PIC disable
- [✓] LAPIC timer driver (~625 Hz)
- [✓] Serial console (COM1, 115200 baud 8N1)


## Phase 2: System Infrastructure (COMPLETED)

Status: 100% complete

Task Management

```~~~~~~~~~~~~
 * [✓] Task control block (TaskCB) structure
 * [✓] Async task creation using Rust futures
 * [✓] Cooperative task executor
 * [✓] Scheduler skeleton (not preemptive yet)

Capability System
```~~~~~~~~~~~~~~

- [✓] Cryptographic capability handle generation
- [✓] CapabilityStore with HashMap backend
- [✓] Capability types (Memory, Process, IPC, Interrupt, FileDescriptor)
- [✓] Grant/revoke operations

Syscall Interface

```~~~~~~~~~~~~~~
 * [✓] SYS_WRITE (1) - Write to file descriptor or framebuffer
 * [✓] SYS_READ (0) - Read from keyboard buffer
 * [✓] SYS_EXIT (60) - Terminate task
 * [✓] SYS_YIELD (24) - Cooperative yield

Hardware Detection
```~~~~~~~~~~~~~~~

- [✓] CPUID parsing for CPU information
- [✓] CPU topology detection (cores, threads)
- [✓] Hybrid core classification (P-core/E-core) infrastructure


## Phase 3: Hardware Integration (IN PROGRESS)

Status: ~40% complete

VirtIO Block Driver (PARTIAL)

```~~~~~~~~~~~~~~~~~~~~~~~~~~~
 * [~] PCI device enumeration
 * [~] VirtIO device detection
 * [ ] VirtIO queue setup
 * [ ] Block device I/O operations
 * [ ] Disk read/write syscalls

VFS Foundation (PARTIAL)
```~~~~~~~~~~~~~~~~~~~~~~
 * [✓] INode trait abstraction
 * [✓] RamFile implementation for ramdisk
 * [✓] Basic VFS operations (read, write)
 * [ ] Directory support
 * [ ] /dev/console character device
 * [ ] /dev/null, /dev/zero
 * [ ] Path resolution

IPC Core (PLANNED)
```~~~~~~~~~~~~~~~
 * [ ] Message passing between tasks
 * [ ] Shared memory regions with capability protection
 * [ ] IPC channel creation syscall
 * [ ] Send/receive syscalls

Preemptive Scheduling (PLANNED)
```~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

- [ ] Timer-triggered context switches
- [ ] Task state save/restore (registers, stack)
- [ ] Round-robin scheduler
- [ ] Priority inheritance for capability operations


## Phase 4: POSIX Compatibility (PLANNED)

Status: 0% complete
Target: Q2 2026

Linux ABI Layer

```~~~~~~~~~~~~
 * [ ] Enhanced ELF loader with dynamic linking
 * [ ] Basic libc stubs (open, close, read, write, mmap)
 * [ ] Environment variable support
 * [ ] Command-line argument passing

Threading Support
```~~~~~~~~~~~~~~

- [ ] pthread_create implementation
- [ ] Thread local storage (TLS)
- [ ] pthread_join and detach
- [ ] Mutex and condition variables

Filesystem Operations

```~~~~~~~~~~~~~~~~~~
 * [ ] POSIX file API (open, close, read, write, seek)
 * [ ] File descriptor table per task
 * [ ] Capability-gated file access
 * [ ] Standard file descriptors (0=stdin, 1=stdout, 2=stderr)

Signal Handling
```~~~~~~~~~~~~
 * [ ] Signal delivery via IPC
 * [ ] Signal handler registration
 * [ ] Default signal handlers (SIGKILL, SIGTERM, SIGSEGV)
 * [ ] Signal masks


## Phase 5: Optimization & Tools (PLANNED)
Status: 0% complete
Target: Q3 2026

Scheduler Optimization
```~~~~~~~~~~~~~~~~~~~

- [ ] Weighted Fair Queueing (WFQ) implementation
- [ ] CPU affinity and load balancing
- [ ] Cache warmth tracking for migration decisions

IOMMU Protection

```~~~~~~~~~~~~~
 * [ ] IOMMU initialization (Intel VT-d)
 * [ ] DMA memory isolation
 * [ ] Safe zero-copy IPC with IOMMU remapping

Debugging Infrastructure
```~~~~~~~~~~~~~~~~~~~~~~

- [ ] GDB stub (serix-dbg)
- [ ] Kernel panic backtraces with symbol resolution
- [ ] Debug syscall for userspace debugging
- [ ] Core dump generation

Performance Profiling

```~~~~~~~~~~~~~~~~~~~
 * [ ] Context switch latency measurement
 * [ ] Syscall overhead profiling
 * [ ] Memory allocator statistics
 * [ ] Interrupt handler timing


## Phase 6: Release Preparation (PLANNED)
Status: 0% complete
Target: Q4 2026

Userspace Shell
```~~~~~~~~~~~~
 * [ ] Minimal interactive shell (serix-sh)
 * [ ] Built-in commands: ls, cat, echo, ps, kill
 * [ ] Command execution via fork/exec
 * [ ] Pipeline support

CI/CD Pipeline
```~~~~~~~~~~~
 * [ ] GitHub Actions workflow
 * [ ] Automated QEMU boot tests
 * [ ] Miri undefined behavior checks
 * [ ] Kani formal verification for critical paths

Documentation
```~~~~~~~~~~
 * [✓] Getting started guide (README.md)
 * [✓] Contributor handbook (CONTRIBUTING.md)
 * [✓] API documentation (docs/)
 * [ ] User manual
 * [ ] Syscall reference card

Release Engineering
```~~~~~~~~~~~~~~~~
 * [ ] v0.1.0 release with bootable ISO
 * [ ] Demo applications (HTTP server, terminal emulator)
 * [ ] Release notes and changelog
 * [ ] Public announcement


## Key Milestones

**Completed Milestones**
**Upcoming Milestones**

```

```

## Contributing
See CONTRIBUTING.md for how to contribute to these roadmap items.
High-priority tasks for contributors:

 1. VirtIO block driver completion (Phase 3)
 2. Directory support in VFS (Phase 3)  
 3. IPC message passing (Phase 3)
 4. Preemptive scheduler (Phase 3)
 5. Test infrastructure and CI/CD (Phase 6)

Contact the maintainers via GitHub issues to claim a task or propose
new roadmap items.
