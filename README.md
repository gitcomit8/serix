# Serix

Serix is an experimental x86_64 operating system written in `#![no_std]` Rust. It uses a Limine boot flow, a Ring 0 kernel, and a small Ring 3 userspace. See the [development roadmap](docs/ROADMAP.md) for work organized around boot and runtime invariants.

## Implementation status

“Implemented & Tested in QEMU” means the capability has an implementation and its documented QEMU validation has passed. “Partial” means some path exists but the named subsystem or its acceptance criteria are incomplete. “Planned / Stubbed” means there is no complete runnable implementation.

| Subsystem | Status | What runs today |
|---|---|---|
| APIC | **Implemented & Tested in QEMU** | Local APIC, I/O APIC routing, legacy PIC masking, LAPIC periodic timer, and x2APIC mode when CPUID exposes it, including fixed-IPI send with acknowledged per-CPU mailboxes. PIT/HPET are not used as the runtime timer. |
| PIT / HPET | **Planned / Stubbed** | No PIT or HPET timer driver; scheduling ticks use the LAPIC timer. |
| SMP AP startup | **Implemented & Tested in QEMU** | Limine MP starts AP callbacks; APs initialize per-CPU state, per-CPU WFQ run queues, and priority inheritance, then enter the idle loop. Maximum configured per-CPU slots: 16; work stealing is not validated. |
| PCI enumeration | **Implemented & Tested in QEMU** | Configuration-space scan enumerates PCI devices, supplies VirtIO block discovery, and provides bounded MSI-X layout metadata for NVMe/xHCI inventory. MSI-X is not enabled or assigned, and there is no hotplug or resource manager. |
| ACPI | **Partial** | RSDP/root-table validation plus MADT, FADT, and DMAR decoding are used for boot-time topology discovery. Not yet used for APIC routing, power control, or IOMMU programming. |
| Capability enforcement | **Partial** | Per-task capability spaces with `(slot, generation)` references, a central per-syscall authorization table, transitive revocation, validation epochs, and a bounded audit ring at `/proc/serix/cap-audit`. File, memory, and IPC send/receive paths validate authority; process-control and port-creation paths are not fully gated yet. |
| LES policy bridge | **Implemented & Tested in QEMU** | Explicit allow/deny path rules from `/etc/serix/policy.toml` are evaluated in-kernel; unmatched paths and no-policy boots default to allow. `policyd` answers queries when no local snapshot is loaded, and `SYS_POLICY_RELOAD` swaps snapshots transactionally. Audit pagination and device/IPC rules are pending. |
| VFS & mount table | **Implemented & Tested in QEMU** | `BTreeMap` mount table with longest-prefix resolution, `RamDir`/`RamFile` nodes, whole-disk and GPT partition block-device inodes (`/dev/sda`, `/dev/sdb`, `/dev/sdXN`), and auto-mount of the detected filesystem from disk 0 at `/mnt` during boot (RamDir fallback). |
| Block layer & partitions | **Implemented & Tested in QEMU** | A table of up to 8 VirtIO-blk devices, each with its own IRQ vector and ISR status address, replaces the old single-device singleton. `fs/src/gpt.rs` validates the GPT header and entry array by CRC32 and rejects corrupt tables whole; `PartitionDev` presents each partition as an offset `BlockDev`. Only the primary GPT is read; overlap checks, MBR fallback, and write-back of partition tables are not implemented. |
| FAT32 | **Implemented & Tested in QEMU** | BPB parsing, cluster-chain traversal and allocation, 8.3 + LFN directory entries, file read/write, `mkdir`, and `unlink`; images written by Serix were validated with Linux mount interop. |
| Page cache & mmap | **Partial** | A 256-page LRU page cache is allocated at boot, and VMA demand paging/CoW is handled on page faults. The cache is not yet used by the FAT32/ext4 read/write paths and has no unified writeback. |
| Driver Manager | **Planned / Stubbed** | No userspace driver manager or Ring 3 driver-server framework. VirtIO-blk initialization is orchestrated in kernel code. |
| BlockDaemon | **Planned / Stubbed** | No BlockDaemon. The VirtIO-blk driver runs in Ring 0 and exposes a VFS block-device inode. |
| NetDaemon | **Planned / Stubbed** | No network driver, network daemon, or TCP/IP stack. |
| InputDaemon | **Planned / Stubbed** | No InputDaemon. PS/2 keyboard input is handled in Ring 0. |
| ext4 | **Work-in-Progress / Partial** | `ext4d` is a Ring 3 process with kernel VFS IPC proxies, extents, JBD2 journal writeback/recovery, and HTree directory indexing. Interoperability, journal replay after interrupted writes, and malformed-metadata rejection are not acceptance-tested. Do not treat ext4 as production-ready. |

These statuses describe the implementation in this repository, not a compatibility or stability promise. See the [roadmap](docs/ROADMAP.md) for phase goals and completion criteria.

## Architecture at a glance

```text
Ring 3 userspace                         Ring 0 kernel
┌──────────────────────┐                 ┌────────────────────────────┐
│ ext4d (partial)      │◄── IPC ────────►│ syscall entry, capability  │
│ policyd              │   queued ports  │ checks, IPC, scheduler,    │
│ kshell / user tasks  │                 │ VFS core, ext4 VFS proxy   │
└──────────────────────┘                 │ PCI + VirtIO-blk + PS/2    │
                                         └──────────────┬─────────────┘
                                                        │ DMA/MMIO
                                                  hardware / QEMU
```

IPC is port-based. `send` enqueues a fixed-size message; receive can be non-blocking or block the receiver until a message arrives. A direct-message fast path exists for a waiting receiver, but the API is not synchronous rendezvous: a sender does not wait for the receiver to accept the message. Notifications use a separate event mechanism.

There is currently no userspace DMA or MMIO delegation to BlockDaemon or NetDaemon: neither daemon exists. VirtIO-blk BAR access and DMA queue/buffer setup happen in Ring 0; the current DMA address model uses HHDM physical addresses and has no IOMMU isolation. ext4 parsing/operations run in the `ext4d` Ring 3 process when invoked through the VFS proxy; the kernel retains the VFS core and proxy stubs.

“Hybrid” alone can describe different privilege layouts. Serix's current concrete split is the one above: core scheduling, memory, IPC, VFS dispatch, interrupt handling, PCI enumeration, and VirtIO-blk are in Ring 0; `ext4d` and `policyd` are userspace processes. Many planned userspace servers are not implemented.

## Build and run

Requirements:

- Rust nightly with the `x86_64-unknown-none` target and `rust-src` component. The pinned channel is `nightly` in [`rust-toolchain.toml`](rust-toolchain.toml); install with `rustup toolchain install nightly --component rust-src --target x86_64-unknown-none`.
- GNU Make, Git, QEMU (`qemu-system-x86_64`), and `xorriso`.
- `mkfs.ext2` for the default `disk.img` target; `mkfs.ext4` to create the optional ext4 image.

```sh
git clone https://github.com/gitcomit8/serix.git
cd serix
make run
```

`make run` builds the kernel and Limine ISO, creates a disk image if needed, and launches QEMU with serial output on the terminal. The first build clones and builds Limine from its `v10.x-binary` branch. `make iso` builds the ISO without launching QEMU. Successful boot should produce initialization messages on serial and a framebuffer console.

For UEFI testing, the ISO includes Limine's UEFI boot image. Install OVMF from your host distribution and pass its firmware code image using QEMU's `-bios` option; firmware paths vary by distribution.

## Documentation

- [Roadmap](docs/ROADMAP.md) — phases, invariants, status, and acceptance checks
- [Architecture](docs/ARCHITECTURE.md) — privilege split, IPC, and subsystem boundaries
- [Boot process](docs/BOOT_PROCESS.md)
- [Contributing](CONTRIBUTING.md)

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE). See [LICENSE](LICENSE) for the licensing notice.
