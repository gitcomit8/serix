# Serix Development Roadmap

> **Last Updated:** 2026-11-14 · **Target Architecture:** x86_64 (AMD64) · **Current Release:** v0.0.9 · **Next Milestone:** v0.1.0 "Live Userspace CLI"

This document defines the phased development plan for the Serix hybrid kernel. Each phase specifies concrete deliverables, acceptance criteria, and subsystem dependencies. Phases are ordered by the project's critical path; no calendar estimates are provided.

**Direction (updated 2026-11-14).** The project is shifting from breadth-first subsystem work to proving one end-to-end boot path. The near-term target is the **v0.1.0 "Live Userspace CLI"** milestone specified in [USERLAND.md](USERLAND.md): boot the live ISO, attach a payload disk, and land in a Ring 3 `rsh` that starts unprivileged programs from a read-only ext4 userland, with a Ring 3 init process owning the boot sequence instead of the kernel. [VISION.md](../VISION.md) and [LINUX_REPLACEMENT_ROADMAP.md](LINUX_REPLACEMENT_ROADMAP.md) define the longer-horizon target of a credible general-purpose OS; they do not change the near-term order of work.

For architectural context on the subsystems referenced below, see [ARCHITECTURE.md](ARCHITECTURE.md).

## Current Status (v0.0.9)

The kernel has completed Phases 1–2 and the four core features of Phase 3. Current operational capabilities:

- **Boot:** Limine v10.x (BIOS + UEFI), higher-half kernel with HHDM at `0xFFFF_8000_0000_0000`
- **Interrupts:** LAPIC + I/O APIC fully operational; legacy PIC disabled; LAPIC timer at ~625 Hz (vector 49); PS/2 keyboard (vector 33); VirtIO block (vector 34, IRQ 11); x2APIC when CPUID exposes it, with fixed-IPI send and acknowledged per-CPU mailboxes for TLB shootdown, scheduler kick, and panic stop
- **Memory:** 4-level paging (PML4), `StaticBootFrameAllocator`, 1 MiB kernel heap (`linked_list_allocator`), SLUB allocator for large objects, and per-task kernel stacks from the pre-mapped `KSTACK_VA_START` region (`0xFFFF_B000_0000_0000`; 1 MiB for spawned processes, 64 KiB for kshell, 4 KiB for APs)
- **Scheduling:** Preemptive per-CPU Weighted Fair Queueing (WFQ) with virtual-runtime tracking and priority inheritance; LAPIC timer invokes `schedule()` at ~625 Hz; `TaskCB` with pre-mapped kernel stacks; callee-saved GPR + CR3 context switch; `block_current_and_switch()` for blocking primitives
- **Syscalls (29 total):** `SYSCALL`/`SYSRET` via MSR; Serix-native numbers (not Linux-compatible): Process group — `SYS_EXIT(0)`, `SYS_YIELD(1)`, `SYS_GETPID(2)`, `SYS_GETPPID(3)`, `SYS_SPAWN(4)`, `SYS_WAIT(5)`; File I/O — `SYS_OPEN(10)`, `SYS_CLOSE(11)`, `SYS_READ(12)`, `SYS_WRITE(13)`, `SYS_SEEK(14)`, `SYS_DUP(15)`, `SYS_DUP2(16)`, `SYS_PIPE(17)`, `SYS_GETDENTS(18)`; Filesystem — `SYS_MKDIR(20)`, `SYS_UNLINK(21)`, `SYS_RMDIR(22)`, `SYS_FORMAT(23)`; IPC — `SYS_SEND(30)`, `SYS_RECV(31)`, `SYS_RECV_BLOCK(32)`, `SYS_CREATE_PORT(33)`, `SYS_CREATE_NOTIFICATION_PORT(34)`, `SYS_NOTIFY(35)`; Memory — `SYS_MMAP(90)`, `SYS_MUNMAP(91)`; File read — `SYS_FILE_READ(92)`; Policy — `SYS_POLICY_RELOAD(93)`. The central `authorization_for()` table classifies 24 calls as capability-gated and 5 as unprotected; file, memory, and IPC send/receive paths validate through `authorize_current_inode()`/`authorize_current_ipc()`, while process-control arms still rely on task/parent identity (see Phase 6 below).
- **IPC:** Port-based message passing; blocking `receive_blocking()` with wait queues; `send()` wakes blocked receivers; producer/consumer validated
- **Storage:** VirtIO 1.0 block device (PCI modern, two-phase init); virtqueue with DMA-safe HHDM frame allocation; IOAPIC-routed completion interrupt registered when the device exposes one, with polled used-ring completion; `BlockDevice` VFS INode for byte-oriented access; 64 MiB ext2 image used by `make disk`, write→read verified
- **Filesystem:** FAT32 driver (`fs/` crate) with BPB parsing, cluster chain traversal/allocation, directory entry creation (8.3 + LFN), file read/write, `mkdir`, `unlink` (with LFN cleanup), duplicate filename rejection, LAPIC-tick timestamps; FAT32 images tested with `mkfs.vfat -F 32`; files created by Serix were visible when mounting the image on Linux. The current `make disk` target builds a 64 MiB ext2 image by default
- **Ext4 (Ring 3):** ext4 daemon (`ext4d`) + kernel VFS IPC stubs are wired into boot, with superblock/BGDT/inode/extent support, IPC-backed `lookup`, `readdir`, `read`, `write`, `mkdir`, `create`, `unlink`, JBD2 journal writeback/recovery on the mutating handlers, and HTree directory indexing (creation + lookup); boot probes `/dev/sda` and auto-mounts the detected filesystem at `/mnt` with RamDir fallback
- **File Descriptors:** Global FD table (`kernel/src/fd.rs`) keyed by `(task_id, fd)`; `open()`/`close()`/`seek()` operations; FDs 0-2 backed by stdio INodes (fd 0 → PS/2 keyboard, fd 1 → framebuffer console, fd 2 → serial); user files start at fd 3
- **Subsystems:** VFS (ramdisk + RamDir/RamFile/BlockDevice INodes, longest-prefix mount matching), ELF loader (PT_LOAD, PT_GNU_STACK), IPC, async executor, capability store with per-task cspace and authorization-gated syscalls, LES (Least-Elevated Security) bridge with declarative policy engine + audit ring, PCI enumeration, serial + framebuffer console, fs (FAT32), /proc/serix/cap-audit pseudo-file, VMA manager with SYS_MMAP/MUNMAP, ACPI MADT/FADT/DMAR discovery, and the Ring 3 `policyd` policy daemon

### Known limits that shape the next milestone

These are the gaps that currently prevent the kernel from delivering a usable userland session, in order of impact:

- **Single block device:** `drivers/src/virtio.rs` holds `VIRTIO_BLK: Once<Mutex<VirtioBlock>>` and `store_global()` uses `call_once`, so a second virtio-blk function is initialized and silently discarded. `fs::VirtioBlockDev` is a zero-sized singleton.
- **No partition layer:** there is no MBR/GPT parsing anywhere; every disk is treated as one filesystem and exposed only as `/dev/sda`.
- **No mount syscall:** only the in-kernel `vfs::mount()` exists; userspace cannot mount anything, and the kernel auto-mounts `/dev/sda` at `/mnt` during boot.
- **Kernel owns boot:** `init_scheduler()` spawns `/ext4d`, `/policyd`, and `kshell` itself and synthesizes `/etc/serix/policy.toml` in RAM. There is no `init` binary and no service supervision.
- **Spawn cannot pass arguments:** `SYS_SPAWN(4)` takes only a path; `spawn_user_process(path, parent_id)` builds a stack with no `argc`/`argv`/`envp`.
- **Handles have no usable generation:** `SYS_OPEN` returns a bare fd while `ulib` hardcodes capability generation 1; there is no `opendir` and no relative open.
- **Enforcement is per-arm, not table-driven:** `authorization_for(nr)` is defined in `kernel/src/syscall.rs` but never called; `SYS_SPAWN` has no authorization check at all.
- **Nothing is reclaimed:** exit closes descriptors and pushes a zombie, but address spaces, page tables, user pages, and kernel stacks are never freed and the frame allocator has no free path.

---

## Next Milestone: v0.1.0 — Live Userspace CLI

**Status:** In progress (design locked; implementation not started). Full specification: [USERLAND.md](USERLAND.md).

This milestone replaces the old assumption that Phase 8 would arrive after all hardware work. It is now the critical path: a single recorded QEMU session must boot the live ISO, discover a payload disk, start `/sbin/init`, mount the read-only ext4 userland, and reach an `rsh` prompt where external programs start with arguments, exit statuses, inherited descriptors, and real capability enforcement.

**Ownership after the milestone:** the kernel discovers disks, exposes GPT partitions, finds the payload by marker, mounts the FAT32 bootstrap volume at `/`, and spawns exactly one process (`/sbin/init`) with the payload disk path as `argv[1]`. Init mounts the ext4 payload and data disks, starts and respawns `ext4d`, `policyd`, and `rsh`, and loads policy. `kshell` becomes a Ring 0 fallback only.

### Blockers

| ID | Work item | Current state | Done when |
|---|---|---|---|
| **B1** | Multi-device block layer | `Once` singleton, second disk dropped | `/dev/sda` and `/dev/sdb` report distinct capacities and data |
| **B2** | GPT and partition devices | No partition code exists | `/dev/sda1`/`/dev/sda2` read correct offsets; corrupt GPT refused |
| **B3** | `SYS_MOUNT` + read-only ext4 | No mount syscall; `ext4d` always writable | Init mounts payload read-only at `/` and data at `/mnt`; writes to `/` return `EROFS` |
| **B4** | Payload discovery and bootstrap mount | Kernel takes the first disk and mounts nothing at `/` | Marker found, FAT32 bootstrap mounted at `/`, init path validated; bounded retry then `kshell` with a reason |
| **B5** | Init: orchestration, switch root, respawn | No init binary; kernel spawns daemons | Kernel spawns exactly one process; init owns mounts, policy, daemons, and shell respawn |
| **B6** | ext4 hardlinks | No `link()`, link count, or shared-inode handling | `core` and `ls` share an inode, both execute, deleting one name keeps the other |
| **B7** | Open handles: slot, generation, `opendir` | Bare fds; stale generations unknowable | Relative `opendir`/`open` works and stale generations are rejected |
| **B8** | Arguments for spawn | No `argc`/`argv`/`envp` on the user stack | `argv[0]` dispatch works; a test program prints correct args |
| **B9** | Descriptor inheritance | Fresh stdio minted per spawn | Redirection and pipes work across spawn with shared `OpenFile` offsets |
| **B10** | Full authorization enforcement | `authorization_for` never called; `SYS_SPAWN` ungated | Table enforced before dispatch; `rsh` can start `/bin/ls` but not `/sbin/policyd` |
| **B11** | `ulib` and the `rsh` contract | No formatting, args, allocator, or published ABI | A program built against the published ABI runs; `rsh` builds against the same crate |
| **B12** | Lifecycle and reclamation | Address spaces, pages, and kernel stacks never freed | Fifty starts and one shell crash leave free frames within a documented bound |

### Order of work

```text
B1 multi-disk ─> B2 GPT ─> B4 payload discovery ─> B5 init
                                    │
B3 SYS_MOUNT + read-only ext4 ──────┘

B6 hardlinks ─┐
B7 opendir   ├─> usable userland
B8 argv      │
B9 fd inherit┘

B10 full enforcement ── required before B5 is finished
B11 ABI + ulib ─────── required by rsh
B12 reclamation ────── required by the 50-start session item
```

**Immediate next step: B1 and B2.** They are independent kernel-only work, need no userspace ABI decisions, and every later item depends on partitions existing. Land them behind host-built GPT/fixture images added to the QEMU configuration, then proceed to B3 and B4 in parallel.

### Interface decisions to settle before B3/B7/B8/B10

These are interface choices, not consequences of the design, and implementation should not start on the affected items until they are fixed:

1. **Handle return encoding** — pack `(slot, generation)` into one `u64`, or write through an out-pointer. Affects every syscall signature and `ulib`.
2. **Delegation mechanism** — a `SYS_GRANT`/`SYS_DELEGATE` syscall, or a kernel spawn policy table keyed by path. Full enforcement with scoped spawn rights needs one of them.
3. **Spawn call shape** — directory handle plus name, or absolute path plus a `SPAWN`-authorized directory capability.
4. **`ext4d` readiness handshake** — port probe, notification, or blocking first request.
5. **`ulib` ownership** — a published crate in this repo, or vendored into the separate `rsh` repo.

### Explicitly out of scope for v0.1.0

Disk installation, bootloader installation, UEFI disk boot, networking, dynamic linking, `fork`/`execve`, multi-user permission bits, signals, and package management. The live ISO and Limine remain the delivery mechanism.

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
- [x] Pre-mapped kernel stack region (`KSTACK_VA_START`) with per-task stacks: 1 MiB for spawned processes, 64 KiB for kshell, 4 KiB for APs
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
- [x] 64 MiB ext2 image (131072 sectors) created by `make disk`, write→read verified via VFS interface
- [ ] Ring 3 driver server process with MMIO BAR mapped into userspace (Phase 7)

---

## Phase 4: Storage & Filesystem Stack

**Status:** In progress. JBD2 journaling and HTree indexing are implemented, but the listed recovery/coherence acceptance tests have not been added and passed.

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
- [x] Directory entry parsing (linear + HTree/dx_root indexed) — `lookup_htree()` traversal plus automatic conversion at the entry-count threshold (`fs/src/ext4/htree.rs`)
- [x] File read path: inode → extent lookup → VirtIO-blk sector read
- [x] File write path: block allocation from bitmap, extent tree insertion, data writeback
- [x] `rmdir()` syscall (`SYS_RMDIR(22)`) — validates METADATA rights on parent, delegates to `inode.rmdir()`
- [ ] Stricter ext4 inode/link-count checks in ext4d daemon
- [x] `SYS_FORMAT(23)` — formats a VirtIO-blk device with ext4 filesystem (block size, inode count derived from device capacity); requires `PLATFORM_ADMIN` right on a block device capability
- [ ] Superblock generation and formatting via CLI from kshell (`format /dev/sda` builtin)
- [x] JBD2 journal — transaction begin, metadata buffering, commit, and startup crash recovery (`fs/src/ext4/journal.rs`), wired into the `ext4d` mutating IPC handlers
- [ ] Journal acceptance testing — interrupted-write replay and Linux `e2fsck` interoperability

### Page Cache & Mappings

- [x] `PageCache` keyed by `(inode_id, page_offset)` in a `BTreeMap` with LRU eviction, dirty tracking, and explicit writeback helpers; 256 pages initialized at boot
- [ ] Wire the page cache into the FAT32/ext4 read/write paths — only boot initialization calls `init_page_cache()`, and no filesystem path uses it
- [x] Demand paging: the `#PF` handler calls `memory::vma::handle_page_fault()` and file-backed faults load pages through the backing inode
- [x] `mmap()` file-backed mapping support (`MAP_SHARED` and `MAP_PRIVATE` with CoW); `cow_fault()` on write fault to private mappings
- [ ] Unified writeback via PTE accessed/dirty bits and deferred flush — helpers exist, but nothing tracks or flushes dirty PTEs yet

---

## Phase 5: LES (Least-Elevated Security) Policy Bridge

**Status:** Implemented and running in QEMU. `policyd` is spawned at boot with TOML config, hot-reload, and the LES bridge evaluates path rules; audit pagination and device/IPC rules remain.

> **Clarification:** "LES" in Serix stands for **Least-Elevated Security**, not Linux Executable Subsystem. It is a declarative access control bridge, not a Linux ABI translation layer.

### LES Policy Engine (Implemented)

- [x] `les_bridge` module (`kernel/src/les_bridge.rs`) with atomic `PolicySnapshot` swap for transactional policy reload
- [x] Local deny/allow rules evaluated at syscall entry: default-allow policy, explicit deny paths
- [x] Default deny rules at boot: `/etc/shadow`, `/proc/serix/cap-audit`
- [x] Policy daemon fallback: kernel queries Ring-3 `policyd` (port 0x5000) when no local policy snapshot is loaded; unmatched local rules default to allow
- [x] `PolicyRule` type with path, `MatchType` (`Exact`/`Prefix`), `PolicyAction` (`Allow`/`Deny`), and required capability rights
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

- [x] File, memory, IPC send/receive/notify, and filesystem-mutation syscalls validate capability authority before observable state changes:
  - `SYS_OPEN`, `SYS_READ`, `SYS_WRITE`, `SYS_SEEK`, `SYS_CLOSE`, `SYS_DUP`, `SYS_DUP2` → inode authority
  - `SYS_SEND`, `SYS_RECV`, `SYS_RECV_BLOCK`, `SYS_NOTIFY` → IPC port authority
  - `SYS_CREATE_PORT`, `SYS_CREATE_NOTIFICATION_PORT` → mints a fresh capability for the caller instead of consuming pre-existing authority
  - `SYS_MKDIR`, `SYS_UNLINK`, `SYS_RMDIR` → parent directory METADATA authority
  - `SYS_PIPE` → directory CONTROL authority
  - `SYS_MMAP`, `SYS_MUNMAP` → inode MMAP/MUNMAP authority
  - `SYS_FORMAT` → block device PLATFORM_ADMIN authority
  - `SYS_EXIT`, `SYS_YIELD`, `SYS_GETPID`, `SYS_GETPPID` → no authority required
- [x] Per-task capability space (`TaskCB.cspace`: `Arc<Mutex<CapabilitySpace>>`) with slot generation tracking
- [x] Capability store (`CapabilityStore`: `BTreeMap<CapabilityId, CapabilityRecord>`) with `mint_root()`, `delegate()`, `revoke()`
- [x] `authorize_current_inode()` / `authorize_current_ipc()` central authorization helpers
- [x] Exhaustive `authorization_for(nr)` table classifying every dispatch arm as `Unprotected` or capability-gated (24 of 29 calls gated; `SYS_SPAWN`/`SYS_WAIT` still authorize by task/parent identity)
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

**Status:** Basic SMP operational; ACPI discovery, x2APIC, acknowledged IPI mailboxes, and PCI MSI-X inventory added; IOMMU programming, power controls, and isolated device servers remain in progress

### SMP Broadcast & Topology

- [x] Per-CPU data structures (`PerCpuData`) accessed via `GS_BASE` MSR; `PER_CPU_DATA_BASE` set during init
- [x] Per-CPU run queues; `current_cpu_id()` reads LAPIC ID from MSR `0x1B`
- [x] Limine MP callback-based AP bootstrap; BSP signals `AP_READY_PTR` on completion
- [ ] INIT-SIPI-SIPI sequence via LAPIC ICR for AP wake-up (currently relies on Limine to hand off running APs)
- [x] ACPI MADT parsing (checksum-validated) for platform topology discovery; AP LAPIC IDs still come from the Limine MP response
- [x] ACPI RSDP/root/SDT validation and MADT/DMAR/FADT decoding (discovery diagnostics; not yet wired into APIC routing or controllers)
- [x] `x2APIC` mode enable (MSR-based, no MMIO) when CPUID indicates support; QEMU `-cpu max` boot verified
- [x] xAPIC/x2APIC register access selection, per-CPU mode setup, APIC ID reads, and fixed IPI send primitive
- [x] Fixed-IPI send plus TLB-shootdown, scheduler-kick, and panic-stop handlers with bounded per-CPU mailboxes and acknowledgement timeout (`apic/src/ipi.rs`)
- [ ] Wire TLB shootdown into VMA reclamation and scheduler kick into run-queue wakeups
- [ ] BORE scheduler policy, only after SMP, per-CPU scheduling, and IPI support have stabilized; WFQ remains the Phase 3 baseline.

### IOMMU (Intel VT-d / AMD-Vi)

- [x] ACPI **DMAR** table parsing (DMA Remapping Hardware Unit Definition structures) for discovery and diagnostics
- [ ] IOMMU page table construction (4-level, analogous to CPU paging)
- [ ] Per-device DMA domain isolation — restrict each PCIe function's DMA to allocated frame ranges
- [ ] Interrupt remapping via IOMMU Interrupt Remapping Table (IRT) to prevent MSI injection attacks
- [ ] Fault logging: IOMMU fault events surfaced to Server Manager via IPC

### Power Management

- [x] **ACPI FADT** parsing (`PM1a_CNT_BLK`, `RESET_REG` fields) for discovery
- [ ] Power off/reboot through ACPI S5/reset (kshell currently halts or triple-faults)
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

**Status:** Re-scoped. The first MVP is the **v0.1.0 Live Userspace CLI** milestone above; the items in this phase are the longer-horizon expansion that follows it.

This phase targets a **Minimum Viable Product (MVP)** demonstrating the full kernel stack end-to-end. It no longer waits on all hardware enablement (Phase 7); the milestone above is the active path.

### Userspace Shell (`rsh`)

> **Note:** Serix currently has `kshell`, a kernel-resident Ring-0 shell with commands: `help`, `echo`, `logs`, `ls`, `cat`, `clear`, `policy`, `write`, `mkdir`, `rm`, `mount`, `umount`, `halt`, `reboot`, plus output redirection (`>` and `>>`). `rsh` is a planned Ring-3 userspace shell.

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
| **4** | Storage & Filesystem Stack (FAT32, ext4, mount table) | ✅ FAT32 complete; ext4 daemon with JBD2 journal + HTree indexing; mount table and boot auto-mount implemented |
| **5** | LES (Least-Elevated Security) Policy Bridge | ✅ Policy engine + audit ring + `policyd` integration; hot-reload implemented; audit pagination pending |
| **6** | Security Bridge & Capability Enforcement | 🔄 Core enforcement operational (24 of 29 syscalls classified capability-gated); process-control gating and adversarial validation pending |
| **7** | Hardware Enablement (SMP, IOMMU, ACPI, NVMe, XHCI) | 🔄 SMP operational; ACPI MADT/FADT/DMAR decoding, x2APIC, IPI mailboxes, MSI-X inventory; IOMMU/power/NVMe/XHCI pending |
| **0.1.0** | **Live Userspace CLI** (multi-disk + GPT, `SYS_MOUNT`, init, `rsh`, argv, enforcement) | 📋 **Next.** Design locked in [USERLAND.md](USERLAND.md); blockers B1–B12 |
| **8** | Userspace & MVP Deliverables (rsh shell, /proc, demo) | 📋 Re-scoped: follows the v0.1.0 milestone (kshell Ring-0 shell exists with 14 commands) |
| **9** | Networking (VirtIO-net, zero-copy, TCP/IP) | 📋 Planned |
| **10** | Optimization & Tooling (perf, debug, CI/CD) | 📋 Planned |

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for development guidelines. High-priority items for contributors:

1. **Multi-device block layer + GPT (B1, B2)** — replace the `VIRTIO_BLK` singleton with a device table and expose `/dev/sdXN` partitions from a validated GPT; the first step toward the v0.1.0 milestone (see [USERLAND.md](USERLAND.md))
2. **`SYS_MOUNT` and read-only ext4 (B3)** — a userspace mount path with `MS_RDONLY`, so init rather than the kernel owns the root
3. **Spawn arguments and handle generations (B7, B8)** — `argc`/`argv`/`envp` on the user stack and `(slot, generation)` handles with relative `opendir`/`open`
4. **Table-driven capability enforcement (B10)** — actually call `authorization_for(nr)` before dispatch and gate `SYS_SPAWN`, then add adversarial QEMU tests
5. **Ext4 acceptance validation** — image interoperability, journal replay after interrupted writes, and malformed-metadata rejection (Phase 4)

File issues or open draft PRs on [GitHub](https://github.com/gitcomit8/serix) to claim a task.
