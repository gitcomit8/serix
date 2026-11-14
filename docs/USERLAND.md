# Serix Userland

> **Status:** design proposal, decisions locked · **Last Updated:** 2026-11-14 · **Milestone:** Live Userspace CLI

This file defines the userland Serix presents on a live boot: the disk layout that carries it, the root layout programs see, the program ABI, who owns boot orchestration, and the exact work remaining before a session is usable.

The goal is **not** installation. The bootloader and live ISO workflow stay as they are; the payload travels on a second disk that the machine reads at boot.

---

## 1. The goal

> **Boot the live ISO, mount a payload disk, and land in `rsh`: a shell running in Ring 3, starting unprivileged programs from a read-only ext4 userland, with init owning the boot sequence instead of the kernel.**

Today `kshell` is a Ring 0 kernel task; every command is a match arm, and the only userspace programs are two daemons embedded in the kernel image. The target is the opposite: the kernel finds a payload disk, starts one init process, and everything else, including the shell and the filesystem daemon, comes from that disk.

### Definition of done

A recorded session that does all of the following on one boot:

1. The payload disk is attached as a second virtio disk; the kernel finds its bootstrap partition by marker and starts `/sbin/init` from it.
2. `init` starts `ext4d`, mounts the ext4 userland partition read-only at `/`, and mounts the data partition at `/mnt`.
3. `init` starts `policyd`, loads `/etc/serix/policy.toml`, and starts `/usr/bin/rsh`.
4. `rsh` reaches a prompt with no kernel shell involvement; `ls /bin` shows `ls`, `cat`, `echo`, and `core` as names for the same inode.
5. `ls /usr/bin`, `ls /`, `ls /tmp`, and `ls /root` resolve to the partitions and RAM directories defined below.
6. `run hello Ayaan` prints a greeting proving `argc = 3` and `argv[1] = "Ayaan"`.
7. `run missing` prints `not found` and returns to the prompt; `echo $?` reports the decoded exit status.
8. Exiting `rsh` restarts it; killing `ext4d` or `policyd` is visible in the log.
9. Removing the payload disk boots to `kshell` after a retry, with a message explaining why.
10. Fifty sequential program starts do not exhaust memory or leave zombies.

---

## 2. Locked decisions

These came from the design interview and are not open for reinterpretation here.

| Area | Decision |
|---|---|
| Directory primitive | `opendir` handles; **no** kernel cwd. `rsh` tracks its own cwd. |
| Executable roots | `/bin` and `/usr/bin`, searched in that order. |
| Utility packaging | Multiplexed core binary plus standalone ELFs allowed. |
| Multiplexing | Real hardlinks, argv[0] dispatch. |
| Payload delivery | Second disk; GPT; ext2 bootstrap + ext4 userland. |
| Payload access | Mounted read-only; never written at runtime. |
| Payload identification | Marker file on the bootstrap partition. |
| Init | `/sbin/init` from the payload; owns shell, daemons, policy, mounts. |
| Shell | `/usr/bin/rsh`, built in a separate repo, integrated later. |
| Shell respawn | Always restart after exit or crash. |
| Spawn rights | Shell may start programs only from `/bin` and `/usr/bin`. |
| Descriptor inheritance | All open descriptors; parent and child share the same `OpenFile`. |
| Exit status | POSIX wait-status encoding, code in bits 8–15. |
| Capability enforcement | Full `authorization_for` table enforced before dispatch. |
| Handle identity | Open returns both slot and generation. |
| Scratch | RAM-backed `/tmp`. |
| Home | RAM-backed `/root`. |
| Devices | `/dev/null` and `/dev/console` in addition to disk nodes. |
| `/boot` | Reserved now, empty, for the future installed layout. |
| Data disk | Separate disk, GPT, one ext4 partition, at `/mnt`; absent is not fatal. |

---

## 3. Boot sequence

### 3.1 Kernel

1. Enumerate PCI and take **all** virtio-blk functions, not just the first.
2. Read the GPT on each disk and expose `/dev/sda`, `/dev/sda1`, ...; partition devices are block devices offset by partition start.
3. Probe each readable partition for the marker `/SERIX.MRK`. The first match is the payload bootstrap volume.
4. Mount that partition read-only at `/`. Verify `/sbin/init` and `/sbin/ext4d` exist and are ELF.
5. Mount the RAM directories at `/dev`, `/proc`, `/tmp`, `/root` so they survive the later root remount.
6. Spawn `/sbin/init` with `parent_id = 0`, passing the boot partition's disk path as `argv[1]`.
7. If no marker is found, retry for a bounded period (late PCI/USB enumeration), then start `kshell` with a serial explanation.

The kernel does **not** mount ext4, spawn `ext4d`, spawn `policyd`, or synthesize a policy file after this milestone.

### 3.2 Init

1. Spawn `/sbin/ext4d` from the bootstrap volume and wait until its request port answers.
2. Mount the payload's ext4 partition (`partition 2` of the same disk, by layout convention) read-only at `/`. The mount call returns a handle for the new root; init keeps it.
3. Spawn `/sbin/policyd` from the ext4 root, read `/etc/serix/policy.toml`, and apply it with `SYS_POLICY_RELOAD`.
4. Locate the data disk: the first GPT disk other than the payload disk with an ext4 partition; mount it at `/mnt`. Failure only logs.
5. Spawn `/usr/bin/rsh` and loop forever: wait for it, log its status, start it again.

### 3.3 Why the bootstrap partition exists

The kernel has no ext4 reader; the ext4 mount path is an IPC stub whose daemon lives on the payload. A small partition that the kernel *can* read breaks that cycle without a second ext4 implementation. Only `/sbin/init` and `/sbin/ext4d` live there.

**Implemented as ext2, not FAT32** (decision 11 in section 11). The kernel already has an ext2 reader (`fs/src/ext2/`), which is the only property this partition needs, and building an ext2 image needs only `mke2fs` — already required by `make disk` — while FAT32 would add `mkfs.fat` and `mtools` to the prerequisites. `make payload` builds the image; see [../payload/README.md](../payload/README.md).

---

## 4. Disk layout

### 4.1 Payload disk

| Partition | Type | Filesystem | Size | Contents | Access |
|---|---|---|---|---|---|
| 1 | EFI System / basic data | FAT32 | 32 MiB | `/SERIX.MRK`, `/sbin/init`, `/sbin/ext4d` | read-only |
| 2 | Linux filesystem | ext4 | remaining | `/bin`, `/usr/bin`, `/sbin`, `/etc`, `/boot` | read-only |

`/SERIX.MRK` is a small text file carrying a layout version, e.g. `serix-payload 1`. It is the only marker the kernel needs, and it lives on the one filesystem the kernel can parse.

Partition 2 is found by convention (same disk, partition 2), which is why the boot partition's disk path is passed to init as `argv[1]`. No partition auto-detection by content is required.

### 4.2 Data disk

| Partition | Type | Filesystem | Contents | Access |
|---|---|---|---|---|
| 1 | Linux filesystem | ext4 | user data, mounted at `/mnt` | read/write |

Any GPT disk that is not the payload disk and has an ext4 partition is eligible. The first one found wins. Missing data disk leaves `/mnt` empty.

### 4.3 Host build

Both disk images are host artifacts: GPT written with a partitioning tool, filesystems created with `mkfs.fat` and `mkfs.ext4`, userland extracted onto the ext4 partition, and the whole thing attached as a second virtio disk in QEMU. The build must be reproducible and its checksum recorded. No kernel-side formatter is required for this milestone.

---

## 5. Root layout

After init finishes, a program sees this tree. Sources differ per path, and only the marked paths are writable.

```
/
├── bin/                  ext4 payload, read-only
│   ├── core              multiplexed implementations
│   ├── ls ─┐
│   ├── cat ├─ hardlinks to core
│   ├── echo┤
│   └── ... ┘
├── usr/
│   └── bin/              ext4 payload, read-only
│       ├── rsh           the shell
│       └── ...           standalone utilities
├── sbin/                 ext4 payload, read-only
│   ├── policyd
│   └── ...               (init and ext4d also exist here in the image)
├── etc/                  ext4 payload, read-only
│   ├── motd
│   └── serix/
│       └── policy.toml
├── boot/                 ext4 payload, read-only, reserved and empty
├── dev/                  RAM, writable
│   ├── null
│   ├── console
│   └── sda, sda1, ...    block devices and partitions
├── proc/                 pseudo-filesystem, read-only
│   └── serix/
│       └── cap-audit
├── tmp/                  RAM, writable, cleared on boot
└── root/                 RAM, writable, cleared on boot
```

| Path | Backing | Writable | State |
|---|---|---|---|
| `/bin`, `/usr/bin`, `/sbin`, `/etc`, `/boot` | payload ext4, mounted read-only | no | missing |
| `/` (mount point itself) | payload ext4 root | no | missing |
| `/dev` | `RamDir` mounted by kernel | yes | exists |
| `/proc/serix/cap-audit` | audit ring pseudo-file | no | exists |
| `/tmp` | `RamDir` mounted by kernel | yes | missing |
| `/root` | `RamDir` mounted by kernel | yes | missing |
| `/mnt` | data ext4, mounted by init | yes | auto-mount exists, ownership moves to init |
| `/dev/null`, `/dev/console` | special inodes | n/a | missing |
| `/dev/sda*` | block devices and partitions | device | whole disks plus GPT partitions |

### Layout rules

1. **Search order is fixed:** `/bin` then `/usr/bin`. No `PATH`; the environment is empty.
2. **No extensions.** `ls`, not `ls.elf`.
3. **Static, non-PIE ELF at `0x200000`**, built with `user.ld`. No interpreter, no dynamic linker, no TLS.
4. **No shebang or script execution.**
5. **Hardlinks are real ext4 links.** `/bin/ls` and `/bin/core` share an inode; argv[0] decides behavior.
6. **Read-only userland.** Programs that need to write use `/tmp`, `/root`, or `/mnt`; writes to `/bin` fail with `EROFS`.
7. **No permission bits.** Read-only is a mount property, not a mode; there are no users yet.
8. **Case-sensitive.**
9. **`/boot` is reserved and empty** so the installed layout later adds files without moving anything.

---

## 6. Program ABI

| Item | Definition |
|---|---|
| Entry | ELF `e_entry` |
| Text base | `0x200000` |
| Stack | 16-byte aligned at entry; `argc` at `[rsp]`, `argv` at `[rsp+8]`, `envp` at `[rsp+16]`, strings above |
| Arguments | `argc`/`argv`/`envp` copied from the parent at spawn; `envp` is empty (`envp[0] = NULL`) |
| Exit | `SYS_EXIT(0..255)`; parent receives wait-status, code in bits 8–15 |
| Capability slots | 0 stdin, 1 stdout, 2 stderr, 3 root directory; new grants append only |
| Open handles | Returned as `(slot, generation)`; both must be presented on every later call |
| Descriptor inheritance | All open descriptors; parent and child share the same `OpenFile`, including offset |
| Stdio | fd 0 keyboard, fd 1 framebuffer console, fd 2 serial; inheritable |
| cwd | None in the kernel. `rsh` holds an opendir handle and resolves relative paths itself |
| Runtime | `ulib`-compatible ABI. `rsh` is built in its own repo and must target this ABI |

Capability slot numbering is load-bearing: `ulib` hardcodes slots 0–5 and generation 1. That conflicts with the decision to return generations, so the constants and the return path must change together.

---

## 7. Who owns what

| Concern | Owner after this milestone |
|---|---|
| Disk discovery, GPT, device nodes | kernel |
| Finding the payload, mounting the bootstrap volume at `/` | kernel |
| Starting `/sbin/init`, fallback to `kshell` | kernel |
| Mounting ext4 payload and data disks | init, via a mount syscall |
| Starting and restarting `ext4d`, `policyd`, `rsh` | init |
| Loading policy from the payload | init |
| Path lookups, capability checks, VFS | kernel |
| ext4 parsing, including hardlinks | `ext4d`, read-only for payload; read/write for `/mnt` |
| Shell behavior, builtins, cwd | `rsh` |

---

## 8. What exists, and what it is worth

| Capability | Where | Status against this design |
|---|---|---|
| Multiple virtio-blk functions | `drivers/src/virtio.rs` | table of up to 8 devices, one IRQ vector and ISR address each |
| Whole-disk and partition nodes | `kernel/src/init/filesystem.rs` | `/dev/sda`, `/dev/sdb`, ... plus validated GPT entries as `/dev/sdXN` |
| GPT parsing | `fs/src/gpt.rs` | primary header only; CRC-validated; backup table, overlap detection, and MBR fallback absent |
| FAT32 read/write | `fs/src/fat32/` | sufficient for the bootstrap volume |
| ext4 read/write through `ext4d` | `fs/src/ext4/` | IPC stubs in kernel; no read-only mount mode, no hardlinks |
| Mount table | `vfs/src/lib.rs` | longest-prefix, replaces on remount, kernel-only API |
| ELF loading | `kernel/src/process.rs` | works from a VFS path; no arguments; path-based grants |
| `SYS_SPAWN` | `kernel/src/syscall.rs` | path only, no authorization check, no argv |
| `SYS_WAIT` | `kernel/src/syscall.rs` | works, wait-status encoding |
| Descriptor handling | `kernel/src/fd.rs` | per-task fds, fresh stdio per spawn, cleanup on exit |
| RamDir/RamFile | `vfs/src/lib.rs` | root, `/dev`, `/proc`; no `/tmp` or `/root` |
| Policy synthesis | `kernel/src/init/filesystem.rs` | kernel writes `/etc/serix/policy.toml` in RAM; must move to payload |
| kshell | `kernel/src/kshell.rs` | Ring 0 fallback only |

---

## 9. Blockers

### B1 — Multi-device block layer

**State.** ✅ Done. `drivers/src/virtio.rs` keeps `BLK_DEVICES: [Once<Mutex<VirtioBlock>>; 8]` and `ISR_CFG_ADDRS`, so each PCI function claims its own slot, IRQ vector (`VIRTIO_BLK_VECTOR + index`), and ISR status address. `store_global()` returns the claimed index, and `fs::VirtioBlockDev::new(index)` carries it into the `BlockDev` layer. `virtio_blk()` is now `virtio_blk_at(0)`.

**Why it blocks.** The payload disk is a second disk by definition.

**Work.**

- [x] Replace the global with a table of devices keyed by a stable id, one per PCI function.
- [x] Make `fs::VirtioBlockDev` carry that id; keep `BlockDev` object-safe.
- [x] Enumerate `/dev/sda`, `/dev/sdb`, ... from the table.
- [x] Add a two-disk QEMU configuration and a test that reads sector 0 of each.

**Done when.** Both disks appear, report distinct capacities, and return distinct data.

**Known limit.** Slots are contiguous, so a failed init in the middle of PCI enumeration hides later devices from `device_count()`. The completion flag is also global to the driver, so concurrent I/O on two disks is not safe; callers currently serialize it.

### B2 — GPT and partition devices

**State.** ✅ Done for the primary GPT. `fs/src/gpt.rs` is a dependency-free parser (host-testable, 11 unit tests) that validates signature, revision, header size, header CRC32, entry-array CRC32, and entry count/size bounds, and rejects a table whole on the first failure. `fs::PartitionDev` presents a partition as a `BlockDev` offset by its start LBA that refuses I/O past its end. Boot publishes `/dev/sdXN` nodes and keeps the whole-disk node.

**Why it blocks.** The payload disk is GPT with two partitions, and the data disk is GPT with one.

**Work.**

- [x] Parse and validate the GPT header: signature, CRC32 of header and entry array, entry count/size bounds.
- [x] Enumerate partitions as `/dev/sdXN`, each a `BlockDev` offset by its start LBA.
- [ ] Reject overlapping, out-of-range, zero-length, and non-`512`-sector-table cases. Out-of-range entries are clipped and zero-length entries skipped, but overlap detection is not implemented and non-512-sector devices are out of scope.
- [ ] Report partition type, start, and length to serial for diagnostics. Start and length are logged; the type GUID is parsed but not printed.
- [x] Keep whole-disk devices addressable so an unpartitioned disk still works.

**Done when.** `/dev/sda1` and `/dev/sda2` exist for a host-created GPT image, read correct offsets, and a deliberately corrupted header is refused.

### B3 — Mount syscall and read-only ext4 mounts

**State.** No `SYS_MOUNT`; only `vfs::mount()` callable from kernel code. `ext4d` has no read-only mode.

**Why it blocks.** Init must mount the payload and data partitions; the kernel must not.

**Work.**

- [ ] `SYS_MOUNT(device_cap, target_path, flags)` returning `(slot, generation)` for the mounted root; `MS_RDONLY` supported.
- [ ] Authorize with `PLATFORM_ADMIN` on the block-device inode; append the new capability without renumbering slots.
- [ ] Teach `ext4d` to mount read-only and reject writes with `EROFS` instead of corrupting the image.
- [ ] Allow replacing the `/` mount while existing handles stay valid.
- [ ] `SYS_UMOUNT` or an explicit refusal to unmount the root.
- [ ] Error paths: busy mount point, unknown filesystem, unreadable superblock, missing device capability.

**Done when.** Init mounts `/dev/sda2` read-only at `/` and `/dev/sdb1` at `/mnt`, writes to `/` fail with `EROFS`, writes to `/mnt` succeed, and a remount of `/` keeps a previously opened file readable.

### B4 — Payload discovery and bootstrap mount

**State.** The kernel takes the first virtio disk, hardcodes `/dev/sda`, and mounts nothing at boot.

**Why it blocks.** Nothing identifies the payload or starts init.

**Work.**

- [ ] Mount each candidate partition read-only in turn and look for `/SERIX.MRK`; unmount candidates that do not match.
- [ ] Validate that `/sbin/init` and `/sbin/ext4d` exist and start with the ELF magic before committing.
- [ ] Mount the chosen volume at `/` and keep the mount for init.
- [ ] Retry the scan for a bounded period, then start `kshell` and print why.
- [ ] Pass the payload disk path as `argv[1]` to init.

**Done when.** With the payload disk attached, serial shows the marker, the partition, and the init path; with it detached, the retry runs and `kshell` starts with the reason logged.

### B5 — Init: orchestration, switch root, respawn

**State.** No init binary; the kernel spawns `ext4d`, `policyd`, and `kshell` itself, and auto-mounts `/mnt`.

**Why it blocks.** The whole ownership model changes: daemons, mounts, policy, and the shell become init's job.

**Work.**

- [ ] New `init` program built with the user build flags, installed at `/sbin/init` on the bootstrap volume.
- [ ] Wait for `ext4d` readiness; a port-answer handshake is needed because the kernel IPC has no readiness concept.
- [ ] Mount payload read-only at `/`, then mount the data partition at `/mnt`; both through `SYS_MOUNT`.
- [ ] Spawn `policyd`, apply `/etc/serix/policy.toml` via `SYS_POLICY_RELOAD`.
- [ ] Spawn `/usr/bin/rsh` and restart it forever; log each exit status.
- [ ] Survive a missing `ext4d` (log and continue to kshell) and a missing data disk (continue).
- [ ] Kernel stops spawning any daemon and stops synthesizing policy.

**Done when.** The session in section 1 runs with the kernel spawning exactly one process.

### B6 — ext4 hardlinks

**State.** No hardlink concept in the VFS or ext4 code: no `link()`, no link count, no shared-inode handling.

**Why it blocks.** `/bin/core` must serve as `ls`, `cat`, `echo`, and the rest through argv[0] without duplicating bytes.

**Work.**

- [ ] `ext4d` read path: return the inode link count, allow multiple directory entries to resolve to the same inode, and keep names distinct.
- [ ] VFS: no change needed for reads, but confirm two paths to one inode both parse as ELF and both spawn.
- [ ] Write path for `/mnt`: `link`/`unlink` with correct link-count decrement and block freeing only at zero.
- [ ] Reject link creation across filesystems and on read-only mounts.
- [ ] A tool or host step that creates the links when building the payload image.

**Done when.** `ls -i`-equivalent reporting shows identical inode numbers for `core` and `ls`, both execute, and deleting one name on the writable data filesystem leaves the other readable with the count decremented.

### B7 — Open handles: slot, generation, and opendir

**State.** `SYS_OPEN` returns a bare fd; capability generations increase on every insert while `ulib` hardcodes 1; directories can be opened but their handles cannot be safely reused. No `opendir`, no relative open.

**Why it blocks.** `rsh` must open `/bin`, list it, and start `/bin/ls`; today a handle's generation is unknowable.

**Work.**

- [ ] Return `(slot, generation)` from open and opendir. Encoding is undecided (see section 11).
- [ ] `SYS_OPENDIR(parent_handle, name)` returning a directory handle; `SYS_GETDENTS` takes one.
- [ ] `SYS_OPEN(dir_handle, name)` for relative opens; absolute form retained for the bootstrap.
- [ ] Path normalization: `.`, `..`, duplicate slashes, trailing slash.
- [ ] Update `ulib` constants and every caller in the same change as the ABI.
- [ ] Reject using a generation that was closed and reused.

**Done when.** A program opens `/`, opendirs `bin`, lists it, opens `ls` relative to it, and repeats after closing and reopening, with stale generations rejected.

### B8 — Arguments for spawn

**State.** The user stack has no `argc`/`argv`/`envp`; `SYS_SPAWN` takes only a path.

**Why it blocks.** `rsh` cannot pass a filename to `cat` or a name to a program, and argv[0] dispatch cannot work.

**Work.**

- [ ] Extend the spawn ABI: path or directory handle plus name, `argv[]`, `envp[]`.
- [ ] Copy user strings into kernel memory with per-string and total caps; validate every pointer.
- [ ] Build the stack: `argc`, `argv[]`, `envp[]`, strings, 16-byte alignment at entry.
- [ ] `envp` empty: one NULL pointer.
- [ ] `ulib`: `args()` and a spawn wrapper taking `&[&str]`.
- [ ] `SYS_SPAWN` must set `argv[0]` to the invoked name, not the resolved path, so hardlinks dispatch.

**Done when.** `/bin/ls` and `/bin/core` invoked as `ls` behave as `ls`, and a test program prints correct `argc` and `argv`.

### B9 — Descriptor inheritance

**State.** Every spawn calls `init_stdio(child_id)` and mints fresh console inodes; nothing is inherited.

**Why it blocks.** The decision is all descriptors, shared `OpenFile`, so redirects, pipes, and descriptor passing work.

**Work.**

- [ ] Copy the parent's fd table at spawn, incrementing `OpenFile` references.
- [ ] Share offsets: parent and child see the same cursor for the same `OpenFile`.
- [ ] Release the reference on close in either process; free at zero.
- [ ] No close-on-spawn flag; document that a program may see descriptors it did not open.
- [ ] Pipes: both ends inherited, with read/write end semantics preserved.
- [ ] Rework `init_stdio` so it is only used for the first process (init) and when a parent genuinely has no stdio.

**Done when.** `ls /bin > /tmp/list` writes the listing through an inherited descriptor, and a pipe to a child delivers data with a shared offset.

### B10 — Full authorization enforcement

**State.** `authorization_for` is defined and never called. Enforcement is per-arm and inconsistent; `SYS_SPAWN` has no check at all; every process gets `LOOKUP|READ` on `/`; grants are keyed by literal paths like `/ext4d`.

**Why it blocks.** The decision is full enforcement, and init must hand the shell rights scoped to two directories.

**Work.**

- [ ] Call `authorization_for` once before dispatch; make exceptions explicit and exhaustive.
- [ ] Define the right required for each arm, including spawn, wait, mount, and the bootstrap grants.
- [ ] Replace path-keyed grants with a real delegation mechanism, or a documented spawn policy table.
- [ ] Grant init `SPAWN`, `WAIT`, `PLATFORM_ADMIN`, and directory rights for `/sbin`, `/bin`, `/usr/bin`.
- [ ] Grant `rsh` `SPAWN` scoped to `/bin` and `/usr/bin` only; verify it cannot start `/sbin/policyd`.
- [ ] Audit every denial path for errno correctness and serial logging.
- [ ] Remove the blanket root `READ` once delegation exists.

**Done when.** `rsh` starts `/bin/ls`, is refused when starting `/sbin/policyd`, and a table-driven test exercises allow and deny for every gated syscall.

### B11 — `ulib` and the `rsh` contract

**State.** `ulib` has syscall wrappers and a `write` helper but no formatting, no arguments, no allocator, and `rsh` lives in another repository.

**Why it blocks.** `rsh` must be written against a stable ABI, or it will be rewritten twice.

**Work.**

- [ ] Publish the ABI: syscall numbers, handle encoding, spawn signature, argument layout, error encoding.
- [ ] Shared bump allocator and `_start` shim with `main(argc, argv)`.
- [ ] `print!`/`println!`, string helpers, `args()`, `spawn`, `wait`, `opendir`.
- [ ] Panic handler that prints and exits nonzero.
- [ ] A hello-world built only against the published ABI, used as the compatibility test for `rsh`.
- [ ] Decide where `ulib` lives relative to the `rsh` repository and how both consume it.

**Done when.** A program compiled against the published ABI runs unmodified, and `rsh` builds against the same crate version.

### B12 — Lifecycle and reclamation

**State.** Exit closes descriptors and pushes a zombie; `SYS_WAIT` drops the `TaskCB`. Address spaces, page tables, user pages, and kernel stacks are never freed; the frame allocator has no free operation.

**Why it blocks.** Init restarts the shell forever and the shell starts a process per command, so leaks accumulate without bound.

**Work.**

- [ ] Frame deallocation or a bounded reclamation path.
- [ ] Tear down the PML4 hierarchy, user mappings, and kernel stack at reap or exit; the orphan path must reach it too.
- [ ] Measure frames across fifty starts; publish the number.
- [ ] Handle a crashed shell without leaking its address space before restart.

**Done when.** Fifty starts and one shell crash leave free frames within a documented bound, with no zombie entries.

---

## 10. Order of work

```
B1 multi-disk ─> B2 GPT ─> B4 payload discovery ─> B5 init
                                     │
B3 SYS_MOUNT + read-only ext4 ───────┘

B6 ext4 hardlinks ─┐
B7 opendir/handles ├─> usable userland
B8 argv            │
B9 fd inheritance ─┘

B10 full enforcement ── required before B5 is considered finished
B11 ABI + ulib ─────── required by rsh
B12 reclamation ────── required by item 10 of the session
```

1. **B1 and B2** are independent kernel work with host-built images as fixtures.
2. **B3** can proceed in parallel with **B4** once B2 exists.
3. **B5** needs B1–B4 and B10; it is the first end-to-end boot.
4. **B6–B9** are what make the session usable; they do not block booting init.
5. **B11** must land before `rsh` integration; **B12** before calling the milestone done.

---

## 11. Not yet decided

These are intentionally left open, because they are interface choices rather than consequences of the decisions above. Implementation should not start on the affected item until they are settled.

1. **Handle return encoding.** Pack `(slot, generation)` into one `u64`, or write through an out-pointer? Affects every syscall signature and `ulib`.
2. **Delegation mechanism.** A `SYS_GRANT`/`SYS_DELEGATE` syscall, or a kernel spawn policy table keyed by path? Full enforcement plus scoped spawn rights needs one of them.
3. **Spawn call shape.** Directory handle plus name, or absolute path plus a SPAWN-authorized directory capability?
4. **`ext4d` readiness handshake.** Port probe, a notification, or a blocking first request?
5. **Bootstrap volume name and marker contents.** `/SERIX.MRK` and `serix-payload 1` are placeholders.
6. **Payload partition number.** Assuming partition 2 on the same disk keeps init simple; a scan would remove the convention.
7. **`rsh` integration contract.** Which syscalls it already uses, and whether its cwd is a handle or a path string.
8. **`ulib` ownership.** Keep it in this repo as a published crate, or vendor it into the `rsh` repo?
9. **Data disk selection.** First non-payload GPT disk, or a marker file on the data partition too?
10. **Core binary argv[0] fallback.** What `core` does when invoked under its own name.

---

## 12. Accepted limitations for v0

- No signals. A hung program cannot be interrupted; recovery is `reboot` from `kshell`.
- No users or permission bits. Read-only means mounted read-only; every process can read everything mounted.
- No persistence outside `/mnt`; `/tmp` and `/root` reset on reboot.
- No networking, no clock beyond timer ticks.
- One console; `rsh` and `kshell` must not both read the keyboard.
- The payload is fixed at build time; changing a utility means rebuilding the disk image.
- `kshell` remains in the kernel as a fallback and bypasses capability checks by construction.

## 13. Non-goals

Disk installation, bootloader installation, UEFI disk boot, networking, dynamic linking, `fork`/`execve`, multi-user, package management. The live ISO and Limine remain the delivery mechanism.
