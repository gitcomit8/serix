# Tier 1 Implementation Plan — Serix Phase 4

**Status:** Approved | **Date:** 2026-05-05 | **Target Completion:** 2026-05-17 | **Effort:** 8-12 days

---

## Overview

Implement the first tier of Phase 4 filesystem support: PCI device enumeration → auto-mount root → multi-filesystem support → improved shell.

**Success Definition:**
- Kernel discovers VirtIO block devices via PCI bus
- `/dev` directory lists discovered devices
- Root filesystem auto-mounts from `/dev/sda` (with fallback to ramdisk)
- Mount table handles multi-filesystem layouts via longest-prefix matching
- Shell provides basic navigation: `ls`, `pwd`, `cd`, human-readable errors

---

## Task Breakdown

### Task 1: PCI VirtIO Enumeration (2-3 days)

**Goal:** Discover block devices on PCI bus, populate device registry

#### 1.1 Extend PCI enumeration to filter VirtIO block devices
- **File:** `drivers/src/pci.rs`
- **What:** Add function to enumerate PCI devices filtered by VirtIO IDs
  - Vendor ID: `0x1AF4` (QEMU)
  - Device ID: `0x1001` (VirtIO block device)
- **Implementation:**
  - Current `enumerate_pci()` scans config space; extend to return type info
  - Create `enumerate_block_devices()` that filters for VirtIO IDs
  - Return Vec of (bus, slot, function, base_address)
- **Acceptance:** Can match VirtIO IDs correctly; log output shows matching devices

#### 1.2 Create BlockDeviceRegistry struct
- **File:** `drivers/src/block_registry.rs` (new file)
- **What:** Global registry to store discovered block devices
- **Structure:**
  ```rust
  pub struct BlockDeviceEntry {
      pub name: String,           // "/dev/sda", "/dev/sdb", ...
      pub index: usize,           // 0, 1, 2, ...
      pub pci_dev: PciDevice,     // (bus, slot, func)
      pub virtio: Arc<VirtioBlock>, // block device handle
  }

  pub struct BlockDeviceRegistry {
      devices: Vec<BlockDeviceEntry>,
  }

  impl BlockDeviceRegistry {
      pub fn register(&mut self, entry: BlockDeviceEntry) -> Result<(), &'static str>
      pub fn get(&self, name: &str) -> Option<Arc<VirtioBlock>>
      pub fn list_all(&self) -> Vec<String>  // returns ["/dev/sda", "/dev/sdb", ...]
      pub fn by_index(&self, index: usize) -> Option<Arc<VirtioBlock>>
  }
  ```
- **Global Instance:** `static BLOCK_REGISTRY: Once<Mutex<BlockDeviceRegistry>> = Once::new()`
- **Export:** Add to `drivers/src/lib.rs`
- **Acceptance:** Registry compiles, methods work in unit tests

#### 1.3 Initialize registry in kernel boot sequence
- **File:** `kernel/src/main.rs`
- **What:** Call PCI enumeration during `_start()`, populate registry
- **Boot Sequence Change:**
  ```
  1. Serial init
  2. APIC init
  3. IDT load + interrupts enable
  4. Heap init
  5. ← NEW: PCI enumeration + registry init (HERE)
  6. VFS init
  7. ext4d spawn
  8. kshell spawn
  ```
- **Implementation:**
  - After heap init, before VFS init
  - Call `drivers::pci::enumerate_block_devices()`
  - For each device found: create VirtioBlock handle, register in BlockDeviceRegistry
  - Log via `serial_println!()`: "Found {count} VirtIO block device(s)"
- **Acceptance:** Serial output shows device count; registry accessible globally

---

### Task 2: /dev Interface (2-3 days)

**Goal:** Create special `/dev` directory that lists discovered devices

#### 2.1 Create DevDirINode (special directory inode)
- **File:** `vfs/src/dev.rs` (new file)
- **What:** INode implementation for `/dev` directory
- **Interface:**
  ```rust
  pub struct DevDirINode;

  impl INode for DevDirINode {
      fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, FsError>
      fn write(&self, offset: u64, data: &[u8]) -> Result<usize, FsError>
      fn readdir(&self) -> Result<Vec<DirEntry>, FsError>  // Query registry
      fn lookup(&self, name: &str) -> Result<Arc<dyn INode>, FsError>  // Find device by name
      fn get_metadata(&self) -> Result<Metadata, FsError>
      // ... other INode trait methods
  }
  ```
- **readdir() Implementation:**
  - Query BlockDeviceRegistry for list of devices
  - Return DirEntry for each (e.g., DirEntry { name: "sda", inode_num: 1, ... })
- **Acceptance:** readdir() returns device list; entries are well-formed

#### 2.2 Create DevFileINode (device file wrapper)
- **File:** `vfs/src/dev.rs`
- **What:** INode implementation for individual device files
- **Interface:**
  ```rust
  pub struct DevFileINode {
      device_name: String,  // "sda"
      handle: Arc<VirtioBlock>,
  }

  impl INode for DevFileINode {
      fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, FsError>
      fn write(&self, offset: u64, data: &[u8]) -> Result<usize, FsError>
      fn get_metadata(&self) -> Result<Metadata, FsError>
      // ... other INode trait methods
  }
  ```
- **read/write:** Delegate to VirtioBlock (raw block I/O)
- **Acceptance:** Can read/write blocks; metadata correct

#### 2.3 Wire into VFS and test
- **File:** `vfs/src/lib.rs`, `kernel/src/main.rs`
- **What:** Mount DevDirINode at `/dev` during boot
- **Implementation:**
  - Create DevDirINode instance
  - Register as `/dev` mount point during VFS init
  - Ensure lookup chain works: `/dev/sda` → DevDirINode.lookup("sda") → DevFileINode
- **Testing:**
  - Boot kernel with kshell
  - Run `ls /dev` in kshell
  - Verify output lists discovered devices (e.g., "sda", "sdb")
- **Acceptance:** `ls /dev` shows device names; can be read (raw blocks)

---

### Task 3: Auto-Mount Root (1-2 days)

**Goal:** Probe `/dev/sda` at boot, auto-detect and mount filesystem

#### 3.1 Add block device access method to registry
- **File:** `drivers/src/block_registry.rs`
- **What:** Expose VirtioBlock handle for VFS mount operations
- **Method:**
  ```rust
  pub fn get_block_device(&self, name: &str) -> Result<Arc<dyn BlockDev>, &'static str>
  ```
- **Where BlockDev is a trait defined in `drivers/`:
  ```rust
  pub trait BlockDev: Send + Sync {
      fn read_block(&self, block_num: u64, buf: &mut [u8]) -> Result<usize, IoError>;
      fn write_block(&self, block_num: u64, data: &[u8]) -> Result<usize, IoError>;
      fn block_count(&self) -> u64;
  }
  ```
- **Acceptance:** Registry returns Arc<dyn BlockDev> correctly

#### 3.2 Implement auto-probe logic
- **File:** `kernel/src/main.rs`
- **What:** Probe `/dev/sda` for filesystem type, mount at root
- **Implementation:**
  ```rust
  fn auto_mount_root() -> Result<(), &'static str> {
      let registry = BLOCK_REGISTRY.get().ok_or("Registry not initialized")?;
      
      // Try to get /dev/sda
      let block_dev = registry.get_block_device("sda")
          .or_else(|_| {
              serial_println!("[BOOT] /dev/sda not found, using ramdisk for root");
              return Err("fallback to ramdisk");
          })?;
      
      // Probe for filesystem type
      let fs_type = fs::probe_filesystem(&block_dev)?;
      serial_println!("[BOOT] Detected filesystem: {:?}", fs_type);
      
      // Mount at root
      match fs_type {
          FsType::Ext4 => {
              // Mount ext4 via ext4d (existing)
              VFS::mount("/", ext4d_inode)?;
          }
          FsType::Fat32 => {
              // Mount FAT32 directly
              VFS::mount("/", fat32_inode)?;
          }
          _ => return Err("unsupported filesystem"),
      }
      
      Ok(())
  }
  ```
- **Called During Boot:** After VFS init, before ext4d spawn (so root is available for ext4d RPC)
- **Acceptance:** Serial output shows detected FS type; `ls /` works post-boot

#### 3.3 Implement graceful fallback
- **File:** `kernel/src/main.rs`
- **What:** If `/dev/sda` missing or unreadable, fall back to ramdisk root
- **Implementation:**
  - `auto_mount_root()` returns `Result<(), &'static str>`
  - If `Err`, continue with ramdisk root (existing behavior)
  - Log fallback reason: serial_println!("[BOOT] Falling back to ramdisk root: {reason}")
- **Acceptance:** Kernel boots successfully with or without disk image

---

### Task 4: Mount Table Verification (1 day)

**Goal:** Verify multi-filesystem support works correctly

#### 4.1 Verify existing implementation
- **File:** `vfs/src/lib.rs` (lines 85-127)
- **What:** Review current mount table implementation
- **Current State:**
  - `MountEntry { path: VirtAddr, root: Arc<dyn INode> }`
  - `MOUNT_TABLE: Vec<MountEntry>`
  - `mount()` adds entry, sorts by path length (longest first)
  - `lookup()` uses longest-prefix matching
- **Acceptance:** Code review complete, no bugs found

#### 4.2 Add dual-disk test setup (optional)
- **File:** `Makefile`
- **What:** Create second disk image for multi-mount testing
- **Implementation:**
  ```makefile
  disk2.img:
      dd if=/dev/zero of=disk2.img bs=1M count=100
      # Format with FAT32
      mkfs.fat -F 32 disk2.img
  ```
  - Update QEMU invocation to include second disk: `-drive file=disk2.img,...`
- **Acceptance:** `ls /dev` shows sda and sdb; can mount both

#### 4.3 Test path resolution across mount points
- **File:** Integration test (manual in kshell)
- **What:** Verify path resolution matches correct mount
- **Test Scenario:**
  1. Mount `/dev/sda` as `/` (ext4 root)
  2. Mount `/dev/sdb` as `/boot` (FAT32)
  3. Verify: `/file` resolves to `/dev/sda`, `/boot/file` resolves to `/dev/sdb`
- **Acceptance:** ls shows correct contents per mount point

---

### Task 5: Shell Improvements (2-3 days)

**Goal:** Make kshell more usable with proper navigation and error handling

#### 5.1 Implement proper `ls` command
- **File:** `kernel/src/kshell.rs`
- **What:** Enhance ls to show file sizes, timestamps, metadata
- **Current:** Likely just lists names
- **Enhanced Output:**
  ```
  $ ls -la /
  drwxr-xr-x root root 4096 Apr 24 15:32 .
  drwxr-xr-x root root 4096 Apr 24 15:32 ..
  -rw-r--r-- root root 1024 Apr 24 15:28 file.txt
  drwxr-xr-x root root 4096 Apr 24 15:30 subdir
  ```
- **Implementation:**
  - Parse command flags (-l, -a, etc.)
  - Query VFS for Metadata (size, perms, time)
  - Format output in columns
- **Acceptance:** `ls` shows useful information; supports common flags

#### 5.2 Add `pwd` command with cwd tracking
- **File:** `kernel/src/kshell.rs`
- **What:** Track current working directory; implement pwd command
- **Implementation:**
  - Add `current_dir: VirtAddr` state in shell context
  - Initialize to root at startup
  - `pwd` prints `current_dir`
  - Use for relative path resolution in other commands
- **Acceptance:** `pwd` shows correct path; used internally for relative paths

#### 5.3 Add `cd` command with relative path support
- **File:** `kernel/src/kshell.rs`
- **What:** Change directory; support relative and absolute paths
- **Implementation:**
  ```
  cd /path       → absolute path
  cd ..          → parent directory
  cd ./subdir    → relative path
  cd ~           → home (root for now)
  ```
- **Algorithm:**
  - Parse path (absolute vs relative)
  - If relative, resolve from `current_dir`
  - Call VFS::resolve() to validate path exists
  - Update `current_dir` on success
- **Acceptance:** `cd` works with various path formats; rejects invalid paths

#### 5.4 Standardize error messages
- **File:** `kernel/src/kshell.rs`
- **What:** Human-readable error output
- **Format:** `command: /path: error message`
- **Examples:**
  ```
  $ ls /nonexistent
  ls: /nonexistent: No such file or directory
  
  $ cd /root
  cd: /root: Permission denied
  ```
- **Implementation:**
  - Map FsError enum variants to human-readable strings
  - Use VirtAddr Display impl to show paths
  - Print to stderr (or stdout if no stderr)
- **Acceptance:** Errors are clear and helpful

#### 5.5 Update shell prompt to show cwd
- **File:** `kernel/src/kshell.rs`
- **What:** Show current directory in prompt
- **Format:**
  ```
  / $ ls
  boot file.txt subdir
  
  /boot $ pwd
  /boot
  ```
- **Implementation:**
  - Modify prompt string to include `current_dir`
  - Use VirtAddr Display impl
- **Acceptance:** Prompt shows correct cwd after cd

---

## Architecture Decisions

### Device Registry Pattern
```
BlockDeviceRegistry (Arc<Mutex<Vec<BlockDeviceEntry>>>)
├── Registered at: drivers::BLOCK_REGISTRY
├── Populated during: kernel boot, after heap init
├── Accessed by: VFS (/dev inode), mount logic, kshell
└── Thread-safe: Yes (Mutex)
```

### /dev Design
```
/dev (DevDirINode)
├── readdir() → queries BlockDeviceRegistry
├── lookup(name) → creates DevFileINode on-the-fly
├── /dev/sda (DevFileINode) → VirtioBlock::read_block()
└── /dev/sdb (DevFileINode) → VirtioBlock::read_block()
```

### Mount Table (Already Implemented)
```
VFS::MOUNT_TABLE (Vec<MountEntry>)
├── Sorted by: path length (longest first)
├── lookup(path) → binary search for longest prefix match
├── mounted filesystems isolated by path
└── Example:
    /          → Ext4 (sda)
    /boot      → FAT32 (sdb)
    /var/log   → Ext4 (sda) - inherits from /
```

### Path Resolution
```
VFS::resolve(path, current_dir) → INode
├── Convert relative to absolute (using current_dir)
├── Split path by '/'
├── Longest-prefix mount table lookup
├── Inode tree traversal from mount root
└── Return final INode or error
```

### Shell State
```
KShell
├── current_dir: VirtAddr = VirtAddr::new(0)  // root
├── dispatch(cmd) → matches command string
├── cd path → validates & updates current_dir
├── ls path → readdir() from that path
├── pwd → prints current_dir
└── Error handling → human-readable messages
```

---

## Testing Strategy

### Unit Tests (per-task)

**Task 1 PCI Enumeration:**
- [ ] VirtIO ID matching (vendor 0x1AF4, device 0x1001)
- [ ] Registry register() / get() / list_all() methods
- [ ] Registry thread safety (Arc<Mutex<>>)

**Task 2 /dev Interface:**
- [ ] DevDirINode readdir() returns registry contents
- [ ] DevFileINode read/write delegates to VirtioBlock
- [ ] lookup() finds device by name
- [ ] Device metadata is correct

**Task 3 Auto-Mount:**
- [ ] probe_filesystem() correctly identifies ext4 vs FAT32
- [ ] auto_mount_root() succeeds with disk present
- [ ] Fallback to ramdisk if disk missing
- [ ] Mount point registered in VFS::MOUNT_TABLE

**Task 4 Mount Table:**
- [ ] Longest-prefix matching works correctly
- [ ] Multi-mount isolation verified
- [ ] Path resolution crosses mount boundaries

**Task 5 Shell:**
- [ ] ls parses arguments and formats output
- [ ] pwd returns correct current_dir
- [ ] cd validates paths and updates state
- [ ] Error messages are human-readable

### Integration Tests (cross-task)

**Boot Sequence:**
- [ ] Kernel boots, PCI enumeration completes, devices listed in serial output
- [ ] `/dev` mounts successfully
- [ ] Root auto-mounts from `/dev/sda` or falls back to ramdisk
- [ ] ext4d spawns and communicates via IPC

**User Workflows:**
- [ ] ls /dev shows devices
- [ ] ls / shows disk contents
- [ ] pwd shows current directory
- [ ] cd /path changes directory
- [ ] Create file, read back, verify content
- [ ] Navigate multi-filesystem layout

**Error Handling:**
- [ ] Missing disk: kernel boots with ramdisk root
- [ ] Invalid path: error message shown, shell continues
- [ ] Permission denied: error message shown
- [ ] Full disk: write fails gracefully

### Build & Test Commands

```bash
# Clean build
make clean && cargo clean

# Format + lint
cargo fmt
cargo clippy

# Build everything (kernel + init binary + ISO)
make iso

# Boot and test
make run

# Boot with debug output
make run-debug  # Adds -d int,cpu_reset -no-reboot

# Check serial output (watch for these lines):
# [BOOT] PCI enumeration complete
# [BOOT] Found 1 VirtIO block device(s)
# [BOOT] /dev mounted
# [BOOT] Detected filesystem: Ext4
# [BOOT] Root mounted successfully
# Spawning ext4d...
# Spawning kshell...
```

---

## Success Criteria

- [x] Plan created (17 subtasks, architecture, testing strategy)
- [ ] Task 1: Serial output shows "Found N VirtIO block devices"
- [ ] Task 2: `ls /dev` lists discovered devices
- [ ] Task 3: `ls /` shows disk contents (or ramdisk fallback works)
- [ ] Task 4: Multi-filesystem paths work correctly
- [ ] Task 5: Shell has pwd, cd, ls with error handling
- [ ] Integration: Full boot sequence works; user workflows succeed
- [ ] All acceptance criteria met per subtask
- [ ] Weekly commits made (2-3 by 2026-05-12)

---

## Key Files to Create/Modify

### Create
- `drivers/src/block_registry.rs` — BlockDeviceRegistry (160 lines)
- `vfs/src/dev.rs` — DevDirINode, DevFileINode (200 lines)

### Modify
- `drivers/src/pci.rs` — VirtIO block filtering (20 lines)
- `drivers/src/lib.rs` — export block_registry module (2 lines)
- `kernel/src/main.rs` — PCI init, auto-mount, registry init (80 lines)
- `kernel/src/kshell.rs` — ls, pwd, cd, error handling (150 lines)
- `vfs/src/lib.rs` — minimal (mount table already exists) (0-10 lines)
- `Makefile` — optional second disk setup (5-10 lines)

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Registry lifetime/scope issues | Unsafe unwrap, null pointer | Use Arc<Mutex<>>, unit tests, graceful errors |
| Path resolution bugs | Wrong filesystem accessed | Longest-prefix test suite, integration tests |
| Circular dependencies (vfs → drivers) | Compilation failure | Define BlockDev trait, crate separation |
| Performance (scanning /dev many times) | Shell slowness | Registry is Vec; O(n) acceptable for MVP |
| ELF segment mapper still breaking | ext4d crash | Verify prior fix holds; monitor boot sequence |
| FAT32 needs refactoring for BlockDev trait | auto-mount fails on FAT32 | Refactor FAT32 to use BlockDev, then mount |

---

## Effort Estimate

| Task | Subtasks | Dev Days | Testing Days | Total |
|------|----------|----------|--------------|-------|
| 1: PCI VirtIO Enum | 3 | 1.5 | 0.5 | 2 days |
| 2: /dev Interface | 3 | 1.5 | 0.5 | 2 days |
| 3: Auto-Mount | 3 | 1 | 0.5 | 1.5 days |
| 4: Mount Table | 3 | 0.5 | 0.5 | 1 day |
| 5: Shell Improve | 5 | 1.5 | 1 | 2.5 days |
| **TOTAL** | **17** | **6** | **3** | **9 days** |

**Actual Range:** 8-12 days (accounts for debugging, refactoring, integration issues)  
**Target Completion:** 2026-05-17 (12 days from start)

---

## Timeline

- **2026-05-05**: Plan approved ✅
- **2026-05-07**: Task 1 complete (2 days dev + test)
- **2026-05-09**: Task 2 complete (2 days)
- **2026-05-10**: Task 3 complete (1.5 days)
- **2026-05-11**: Task 4 complete (1 day)
- **2026-05-12**: CHECKPOINT: Weekly validation, Tasks 1-4 done, 2-3 commits visible
- **2026-05-15**: Task 5 complete (2.5 days)
- **2026-05-17**: Tier 1 COMPLETE, all acceptance criteria met ✅
- **2026-05-31**: Tier 2 complete, Phase 4 ships 🚀

---

## Notes

- **Phase 5 Deferred:** No Linux ABI compatibility until Phase 4 ships
- **Execution Discipline:** Weekly commits required (not "I thought about it")
- **Boot Ordering Critical:** PCI enum → VFS init → ext4d spawn → kshell (not: VFS first then PCI)
- **ELF Loader Fix Verified:** Prior session fixed segment mapping; monitor for regressions
- **No Backwards Compat:** This phase establishes Serix native ABI, not POSIX
- **Init System:** Not needed until Phase 5 (beyond 2026-05-31); userspace stays minimal
- **Cross-Compilation:** Cargo + x86_64-unknown-none target sufficient for MVP

### Now (Phase 4)
- Serix kernel boots and can spawn init binary (ELF loader works)
- Can run **statically-linked Serix-native binaries** compiled with our `x86_64-unknown-none` target
- Init binary (`ulib` examples) demonstrates syscall usage

### Short-term (After Mount Table)
- **Build cross-compiler toolchain**
  - Option A: `cargo build --target x86_64-serix` (custom target JSON, nightly Rust)
  - Option B: Fork musl-libc → serix-libc (C library with Serix syscall stubs)
  - Recommendation: Start with A (simpler), use `x86_64-unknown-none` + `ulib` syscall wrappers
- **Ship ulib as SDK**
  - Publish `ulib` to a registry or include in tarball
  - Provide example Cargo projects that link against ulib
  - Users write Rust code, call `serix_*` functions, compile with Serix target

### Medium-term (If Needed)
- **C toolchain** — GCC with Serix backend (requires binutils + GCC port, high effort)
- **Runtime linker** (RTLD) — dynamic linking support (Phase 5 work, complex)
- **System libc** — full musl-like library, not just syscall wrappers

### Init System Timing

**When needed?** When you want to:
- Multi-process workloads (more than shell + 1-2 daemons)
- Service supervision (restart crashed daemons)
- Boot-time service orchestration (mount order dependencies)

**Current state:** No init system yet. Kernel spawns ext4d + kshell hardcoded in `kernel/src/main.rs`.

**Simple approach (doable now):**
1. Kernel spawns a single `init` binary (instead of ext4d + kshell hardcoded)
2. Init reads `/etc/rc.conf` or similar (plain text file on filesystem)
3. Init spawns services in order (ext4d, kshell, etc.)
4. Init waits for children (simplified reaping without full waitpid())

**Better approach (defer to Phase 5):**
- Full `clone()` syscall (process forking)
- `waitpid()` for reaping
- Service manager pattern (systemd-lite)

## Recommendation: Pragmatic Phase 4 Completion

1. **Implement Tier 1** first (auto-mount, /dev enumeration, mount table) — ~2-3 weeks
   - These are prerequisites for any user-visible FS functionality
   - Foundation for everything that follows

2. **Implement Tier 2** in parallel (shell improvements) — ~1 week
   - Can test Tier 1 work with improved CLI

3. **Ship a working system:** User can boot Serix, see /dev list, mount root disk, create/read/write files

4. **Defer Tier 3 (journal/cache) to Phase 5** — not needed for MVP functionality
   - Add if FS corruption issues arise in testing
   - Optimize if profiling shows I/O bottlenecks

## Binary Support Roadmap

### Phase 4.5 (Immediate)
- [ ] Document ulib syscall ABI
- [ ] Provide example Cargo project template
- [ ] Users can write Rust → compile to Serix binary → run on kernel

### Phase 5 (Next Major)
- [ ] Support basic syscalls needed for any program: `clone()`, `execve()`, `exit_group()`
- [ ] Signal handling (`SIGTERM`, `SIGKILL`, etc.) for process control
- [ ] Then: Higher-level languages (C via musl, Python/Go if compiled statically)

### Future (Phase 6+)
- Runtime linker + dynamic linking
- Full POSIX compatibility layer

## Next Immediate Step

**Implement auto-mount + /dev enumeration** (Tier 1)
- Scan PCI for VirtIO-blk devices
- Assign /dev/sda, /dev/sdb names
- Auto-probe and mount root from /dev/sda
- Update kshell to display /dev contents
- Test: boot kernel, see mounted filesystem, run `ls /`, create/read files
