# Design: stdio FD Allocation + FAT32 Fixes

**Date:** 2026-03-31
**Scope:** Phase 4 completion — standard fd 0/1/2 allocation and FAT32 known-issue resolution

---

## 1. stdio INodes

### Problem

`syscall_dispatcher` hard-codes fd 0/1/2 behavior inline. There are no `FD_TABLE` entries for these fds, so `fd::get(task_id, 0)` returns `None` and `fd::close(task_id, 1)` returns `EBADF`. This breaks any userspace code that inspects or closes stdio.

### Solution

Add `kernel/src/stdio.rs` with three types implementing `vfs::INode`:

| Type | `read()` | `write()` |
|---|---|---|
| `StdinINode` | Spin on `keyboard::pop_key()`, return 1 byte | Returns 0 |
| `StdoutINode` | Returns 0 | `hal::serial_print!` + `graphics::console::_print` |
| `StderrINode` | Returns 0 | `hal::serial_print!` only |

Add `fd::init_stdio(task_id: u64)` which inserts these into `FD_TABLE` at fds 0, 1, 2. Called from `_start` before entering userspace (using task_id 0 for the init process).

Remove the hardcoded `if fd == 0` (stdin) and `if fd == 1 || fd == 2` (stdout/stderr) branches from `syscall_dispatcher`. All read/write routes through `fd::get()` uniformly.

---

## 2. FAT32: Duplicate Detection

### Problem

`FatDirINode::insert()` calls `create_dir_entry` without checking if the name already exists, silently creating duplicate entries.

### Solution

At the top of `FatDirINode::insert()`, call `find_entry_in_dir(bpb, self.cluster, name)`. Return `Err("file exists")` if it returns `Some(_)`.

---

## 3. FAT32: Subdirectory Creation (`mkdir`)

### Problem

Only root-level files can be created. No directory creation path exists.

### Solution

Add to `vfs::INode` trait:

```rust
fn mkdir(&self, _name: &str) -> Result<(), &'static str> {
    Err("not a directory")
}
```

`FatDirINode::mkdir(name)`:
1. Check for duplicate via `find_entry_in_dir`.
2. `fat_alloc_cluster` → allocate new cluster, zero all sectors.
3. Write `.` entry at offset 0 (cluster = new_cluster, attr = `ATTR_DIRECTORY`, size = 0).
4. Write `..` entry at offset 32 (cluster = `self.cluster`, attr = `ATTR_DIRECTORY`, size = 0).
5. `create_dir_entry(bpb, self.cluster, name, ATTR_DIRECTORY, new_cluster)`.

New syscall `SYS_MKDIR = 83`. Args: `arg1`/`arg2` = path ptr/len. Resolves path via `vfs::lookup_path` on the parent component, calls `mkdir(filename)`.

---

## 4. FAT32: File Deletion (`unlink`)

### Problem

No way to delete files. Cluster chains are never freed.

### Solution

Add to `vfs::INode` trait:

```rust
fn unlink(&self, _name: &str) -> Result<(), &'static str> {
    Err("not a directory")
}
```

`FatDirINode::unlink(name)`:
1. `find_entry_in_dir` → get `DirEntry` (entry_sector, entry_offset, first_cluster).
2. Walk cluster chain via `cluster_chain(bpb, first_cluster)`, call `fat_write_entry(bpb, cl, FAT32_FREE)` for each.
3. Mark 8.3 entry byte 0 as `0xE5` via `write_dir_entry`.
4. Scan backward from `entry_offset` within `entry_sector`, marking any preceding LFN entries (attr == `ATTR_LFN`) as `0xE5`.

New syscall `SYS_UNLINK = 87`. Args: `arg1`/`arg2` = path ptr/len. Splits path into parent + filename, resolves parent via `vfs::lookup_path`, calls `unlink(filename)`.

---

## 5. FAT32: Timestamps

### Problem

Directory entries are written with zeroed time/date fields (bytes 14, 16, 22, 24 of the 8.3 entry). No RTC is available.

### Solution

Add `pub fn ticks() -> u64` to `apic::timer`, returning the current LAPIC tick count.

In `create_dir_entry`, after writing the SFN entry, compute a fake FAT32 timestamp:

```
seconds = ticks / 625          // ~1s resolution at 625 Hz
dos_time = ((seconds % 60) / 2)        // 2-second units
         | (((seconds / 60) % 60) << 5)  // minutes
         | (((seconds / 3600) % 24) << 11) // hours
dos_date = 1                   // day=1
         | (1 << 5)            // month=1 (January)
         | (0 << 9)            // year=0 (1980)
```

Write `dos_time` at offsets 14 (creation) and 22 (modified), `dos_date` at offsets 16 and 24. Written as little-endian u16.

---

## 6. DMA Buffer Reuse

### Problem

`read_sector` and `write_sector` call `alloc_dma_page` on every I/O, leaking one 4KiB frame per operation. `StaticBootFrameAllocator` has no `free_frame`, so frames are unrecoverable.

### Solution

Add field to `VirtioBlock`:

```rust
dma_buf: *mut BlkDmaBuffer,
```

Allocate once in `setup_queues` (called after SLUB init) via `alloc_dma_page`. Store the result in this field.

`read_sector` and `write_sector` use `self.dma_buf` directly instead of calling `alloc_dma_page`. Safe because both operations are synchronous — they spin-wait for virtqueue completion before returning, so the buffer is never in concurrent use.

`VirtioBlock` is wrapped in a `Mutex` at the call site, providing mutual exclusion.

---

## Syscall Summary

| Syscall | Number | Args |
|---|---|---|
| `SYS_MKDIR` | 83 | arg1=path_ptr, arg2=path_len |
| `SYS_UNLINK` | 87 | arg1=path_ptr, arg2=path_len |

Both follow the same path-resolution pattern as `SYS_OPEN`.

---

## Files Changed

| File | Change |
|---|---|
| `kernel/src/stdio.rs` | New — StdinINode, StdoutINode, StderrINode |
| `kernel/src/fd.rs` | Add `init_stdio()` |
| `kernel/src/syscall.rs` | Remove hardcoded fd 0/1/2; add SYS_MKDIR, SYS_UNLINK |
| `kernel/src/main.rs` | Call `fd::init_stdio(0)` before userspace launch |
| `vfs/src/lib.rs` | Add `mkdir()` and `unlink()` to INode trait |
| `fs/src/lib.rs` | Duplicate check, mkdir, unlink, timestamps |
| `drivers/src/virtio.rs` | Add `dma_buf` field, allocate once in setup |
| `apic/src/timer.rs` | Expose `ticks()` |
