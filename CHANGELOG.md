# Changelog

## [Unreleased]

### Added

- Per-task x87/SSE state: `CPUContext` carries a 512-byte, 16-byte-aligned `FxSaveArea` saved and restored with `fxsave64`/`fxrstor64` on every context switch (`task/src/context_switch.rs`), with compile-time assertions pinning the hand-written struct offsets. `hal::cpu::enable_sse()` clears `CR0.EM`, sets `CR0.MP`, and sets `CR4.OSFXSR`/`CR4.OSXMMEXCPT` on the BSP and on every AP.
- `ulib/examples/fputest.rs`: Ring 3 stress program whose parent spawns two workers; each worker writes distinct x87, MMX, and SSE patterns, spins long enough to be preempted, and reports mismatches. Built as a naked `_start` that forwards the entry RSP so `argc` reaches the program intact.
- Multi-device VirtIO block layer: `drivers/src/virtio.rs` replaces the `VIRTIO_BLK` singleton with a table of up to 8 devices; each device claims a slot and gets its own IRQ vector (`VIRTIO_BLK_VECTOR + index`) and ISR status address. `store_global()` now returns the claimed index.
- GPT support (`fs/src/gpt.rs`): validated primary-GPT parsing with signature, revision, header-size, header CRC32, entry-array CRC32, and entry-bound checks. A table failing any check is rejected whole. 11 host unit tests cover the parser and every rejection path.
- `fs::PartitionDev`: an offset `BlockDev` that presents a partition's own sector 0 to filesystem drivers and refuses I/O past the partition end.
- Boot enumerates every disk as `/dev/sda`, `/dev/sdb`, ... and every GPT entry as `/dev/sda1`, `/dev/sda2`, ...; disks without a GPT keep only the whole-disk node.
- `make gptdisk` target creates a 64 MiB two-partition GPT image, attached as a second virtio-blk disk by every `make run` variant.

### Changed

- Per-CPU current-task tracking: `task::CURRENT_TASKS` (one slot per CPU) replaces the single global `CURRENT_TASK`, and `current_cpu_id()` reads CPUID leaf 1 `EBX[31:24]` instead of `IA32_APIC_BASE`, which holds no APIC id in its low byte — every CPU previously reported id 0 and shared one run queue, one current-task slot, and one TSS stack hook.
- `fs::VirtioBlockDev` is now `VirtioBlockDev::new(index)` instead of a zero-sized singleton, and `virtio_blk()` delegates to `virtio_blk_at(0)`.
- Boot auto-mount still probes disk 0 only; partition nodes are exposed but not probed or mounted.

### Fixed

- `SYSRET` returned with the user *data* selector (`CS = 0x23`) because `Star::write` was given the data selector for the CS field. The first return to Ring 3 raised `#GP` with error `0x20`; the frame dump showed `[0x2009cc, 0x23, 0x10246, 0x7fff…, 0x1b]`.
- Spawned tasks had empty c-spaces: `init_stdio` minted the stdio capabilities via `find_task_by_id`, which cannot see a child that has not been enqueued yet, so writes to fds 0-2 returned `EBADF`. It now receives the child's `&tcb.cspace` directly, with `CapabilitySpace::insert_with_generation` and `STDIO_GENERATION = 1` preserving generation validation.
- `kshell` ran with interrupts disabled after the `SYS_EXIT` path switched to it: kernel tasks are resumed by `ret` from `context_switch`, which does not restore RFLAGS, so the shell inherited the syscall path's cleared interrupt flag. The LAPIC timer then stopped on that CPU and every queued task starved behind the shell. It now enables interrupts on entry.
- Serial output could wedge the machine: `serial_read_byte()` held the `SERIAL_PORT` lock in a window where it could be preempted, and the next task to print spun on that lock from the syscall path, where interrupts are off, so nothing could resume the holder. The read and log-replay paths now hold their locks with interrupts disabled.

## [v0.0.9] - 2026-11-10

### Added

- Auto-mount ext4 filesystem at boot: `probe_and_mount()` detects ext4 on `/dev/sda` and mounts at `/mnt` after ext4d daemon is spawned, with RamDir fallback if no filesystem is detected.
- ext4 HTree index creation: `create_htree_index()` allocates dx_root header and leaf block for new directories; `maybe_convert_to_htree()` automatically converts linear directories when entry count reaches 3.
- ext4 HTree hash fix: Replaced djb2 variant with ext4 `hash_name_2()` Jenkins one-at-a-time hash with character-dependent bit rotation and proper mask application.
- JBD2 journal writeback: `commit_transaction()` now writes journal header (magic + block count + sequence), all buffered metadata blocks, and commit record.
- JBD2 crash recovery: `recover()` scans journal for valid transaction sequences and replays committed blocks to original disk locations.
- Journal integration in all mutating IPC handlers: MSG_MKDIR, MSG_CREATE, MSG_UNLINK, and MSG_RMDIR now begin transactions, buffer modified inodes, and commit on completion.
- ext4d journal recovery at startup: Daemon calls `journal.recover()` after reading journal superblock.

### Changed

- Mount ext4 at `/mnt` instead of `/` to preserve embedded ELF files (ext4d, policyd) on RamDir for process spawning.
- `/mnt/` directory created during VFS initialization as auto-mount target.
- Fixed `parse_dx_root` and `parse_dx_node`: `info_len` is 1 byte (not 2), corrected field offsets.
- Fixed `parse_dx_node`: `info_len` is 1 byte (not 2), corrected field offsets.

### Fixed

- `hash_name()`: Now uses correct ext4 hash algorithm matching fs/namei.c from Linux kernel.

## [v0.0.8] - 2026-11-04

This release summarizes changes since v0.0.6; it includes the contents of the previously published v0.0.7 release.

### Added

- Kernel-space TTY shell and expanded process, file, and filesystem syscall support.
- SMP startup and per-CPU scheduler queues, weighted fair scheduling, priority inheritance, and TSS kernel-stack switching.
- IPC notification ports and register-mediated fast-path message delivery.
- Ext4 HTree directory lookup, formatting, JBD2 journal support, and stricter directory handling.
- Unified page cache, deferred writeback, demand paging, copy-on-write, and file-backed `mmap` support.
- Per-task capability spaces, generation-checked references, syscall authorization, revocation, delegation, and capability audit logging.
- Ring 3 policy daemon with TOML policy loading and hot reload, integrated through the LES bridge.

### Changed

- Reworked VFS mounts and path canonicalization, and split kernel startup into subsystem initialization modules.
- Migrated userspace syscall wrappers and filesystem services to capability-referenced APIs.
- Updated build configuration and project documentation for the expanded kernel and userspace architecture.

### Fixed

- Corrected SMP per-CPU state setup, APIC handling, and GS-base invariants during syscall/context-switch paths.
- Fixed an audit-ring-related boot crash and preserved capability slot ordering.
- Fixed ext4 HTree hash selection and zero-link-count handling during directory removal.
- Preserved IPC direct-delivery ordering and corrected several filesystem, build, and boot issues.

