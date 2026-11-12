# Serix Development Roadmap

This roadmap tracks work by the runtime invariant each phase must establish. Status describes the repository as it exists today; a checked item is not a claim of production readiness. QEMU acceptance evidence should include the command line and relevant serial output in the change or issue.

## Phase 1: Early Boot & SMP

**Invariant:** the kernel boots through Limine, establishes memory and interrupt state, and brings configured application processors to a known idle state.

- **Implemented & tested in QEMU:** Limine BIOS/UEFI ISO build; higher-half boot and HHDM; Local APIC and I/O APIC initialization; PIC masking; LAPIC timer; Limine MP AP callbacks and per-CPU initialization (up to 16 slots).
- **Partial:** APs initialize and idle, but the scheduler is not a fully validated general-purpose work-stealing SMP scheduler. TSS.RSP0 per-task switching remains incomplete.
- **Planned:** PIT/HPET support and broader hardware coverage.
- **Acceptance:** clean boot serial checkpoints; AP count and AP-ready messages match configured QEMU CPUs; timer interrupts continue without exception or reset.

## Phase 2: Ring 3 IPC & Driver Manager

**Invariant:** user processes can exchange capability-authorized messages, and hardware ownership can be assigned with bounded authority.

- **Implemented & tested in QEMU:** syscall entry/return; ELF loading of static binaries; port-based send and blocking receive; wait-queue wakeup and notification ports; capability checks on IPC; PCI configuration-space enumeration.
- IPC is queued message passing, not synchronous rendezvous. `send` enqueues; a blocked receiver is woken, with a direct-message fast path available.
- **Planned / stubbed:** userspace Driver Manager; Ring 3 driver process framework; delegated BAR mapping and DMA/IOMMU domains. PCI enumeration does not provide hotplug or full resource management.
- **Acceptance:** run producer/consumer and capability-denial scenarios in QEMU; demonstrate that a userspace driver can receive only explicitly granted MMIO/DMA resources before calling this phase complete.

## Phase 3: Storage BlockDaemon & ext4

**Invariant:** VFS storage requests have explicit ownership, predictable failure behavior, and on-disk consistency across restart.

- **Implemented & tested in QEMU:** Ring 0 VirtIO-blk PCI driver; DMA-backed virtqueues; byte-oriented VFS block-device interface; read/write smoke check; FAT32 basic read/write and directory operations.
- **Work-in-progress / partial:** Ring 3 `ext4d` with kernel VFS IPC proxy and a subset of superblock, inode, extent, and file operations. Recovery, directory indexing, and corruption/consistency checks are not fully acceptance-tested.
- **Planned:** BlockDaemon and isolated storage-driver ownership; complete ext4 format/mount/read/write behavior with journal recovery and failure testing.
- **Acceptance:** ext4 image interoperability; create/read/update/delete across reboot; journal replay after interrupted writes; malformed metadata rejection; no claim of complete ext4 until these pass.

## Phase 4: NetDaemon & Userspace ABI

**Invariant:** network and application-facing interfaces are documented, capability-scoped, and testable independently of kernel internals.

- **Planned / stubbed:** NetDaemon, network driver, TCP/IP stack, and network-facing userspace ABI. No network functionality is currently implemented.
- **Planned:** stabilize syscall and IPC wire formats, document compatibility expectations, and add end-to-end user-process validation.
- **Acceptance:** loopback and QEMU virtual-network tests covering address configuration, packet I/O, and process isolation; publish ABI versioning and compatibility policy.

## Status vocabulary

- **Implemented & Tested in QEMU:** code exists and the named runtime path has been exercised in QEMU.
- **Work-in-progress / Partial:** a subset runs, but stated behaviors or acceptance checks remain incomplete.
- **Planned / Stubbed:** design, scaffolding, or a name may exist, but there is no complete runnable path.

The top-level [implementation status matrix](../README.md#implementation-status) summarizes the status of named subsystems. Update both when a subsystem changes status, and include the QEMU evidence used to justify that change.
