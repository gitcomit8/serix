# Block Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a write-through sector-level block cache to the `fs` crate so every ext2 (and future ext4) mount avoids redundant VirtIO round-trips for hot metadata.

**Architecture:** A `CachedBlockDev` newtype wraps any `Arc<dyn BlockDev>`. It keeps a `BTreeMap<u64, [u8; 512]>` of cached sectors plus a FIFO eviction list capped at 512 entries (256 KB). Writes go to the device immediately (write-through) and update the cache entry if present. Reads serve from cache on hit; on miss they read the device and insert the sector. The newtype implements `BlockDev` so it is a drop-in replacement at mount time.

**Tech Stack:** Rust no_std, `alloc::collections::BTreeMap`, `alloc::vec::Vec`, `spin::Mutex`

## Progress Update (2026-04-24)

**Implemented:**
- `fs/src/block_cache.rs` with write-through cached block device
- `fs/src/lib.rs` exports `CachedBlockDev`
- ext2 mount path wrapped with `CachedBlockDev`

**Remaining:**
1. FAT32 path still bypasses `BlockDev` abstraction in mount/runtime I/O (`read_sector` global path), so cache is not yet active there
2. Refactor FAT32 I/O path to use `Arc<dyn BlockDev>` consistently, then wire `CachedBlockDev` at mount
3. Add simple runtime counters (cache hit/miss) or serial instrumentation for validation in QEMU

---

## File Map

| Action  | Path |
|---------|------|
| Create  | `fs/src/block_cache.rs` |
| Modify  | `fs/src/lib.rs` — `pub mod block_cache; pub use block_cache::CachedBlockDev;` |
| Modify  | `fs/src/ext2/mod.rs` — wrap device in `CachedBlockDev` inside `Ext2Driver::mount` |
| Modify  | `fs/src/fat32/mod.rs` — same wrap in `Fat32Driver::mount` |

---

### Task 1: `CachedBlockDev` struct and read path

**Files:**
- Create: `fs/src/block_cache.rs`

- [ ] **Step 1: Write `fs/src/block_cache.rs`**

```rust
/*
 * block_cache.rs - Write-through sector cache
 *
 * Wraps any BlockDev with a fixed-capacity LRU-ish cache.
 * Capacity is measured in 512-byte sectors; eviction is FIFO.
 */

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use crate::BlockDev;

const CACHE_CAP: usize = 512; /* 512 sectors = 256 KiB */

struct Inner {
    cache: BTreeMap<u64, [u8; 512]>,
    order: Vec<u64>,          /* insertion order for FIFO eviction */
    dev:   Arc<dyn BlockDev>,
}

pub struct CachedBlockDev {
    inner: Mutex<Inner>,
}

impl CachedBlockDev {
    pub fn new(dev: Arc<dyn BlockDev>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                cache: BTreeMap::new(),
                order: Vec::new(),
                dev,
            }),
        }
    }

    /* flush - Write all dirty sectors (no-op for write-through; kept for API symmetry) */
    pub fn flush(&self) { /* write-through: nothing to do */ }
}

impl BlockDev for CachedBlockDev {
    fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
        let mut g = self.inner.lock();
        if let Some(line) = g.cache.get(&sector) {
            buf.copy_from_slice(line);
            return true;
        }
        /* Cache miss */
        if !g.dev.read_block(sector, buf) { return false; }
        /* Evict FIFO if full */
        if g.cache.len() >= CACHE_CAP {
            if let Some(old) = g.order.first().copied() {
                g.cache.remove(&old);
                g.order.remove(0);
            }
        }
        g.cache.insert(sector, *buf);
        g.order.push(sector);
        true
    }

    fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool {
        let mut g = self.inner.lock();
        /* Write-through: device first */
        if !g.dev.write_block(sector, buf) { return false; }
        /* Update cache entry if present; insert if there is room */
        if let Some(line) = g.cache.get_mut(&sector) {
            line.copy_from_slice(buf);
        } else if g.cache.len() < CACHE_CAP {
            g.cache.insert(sector, *buf);
            g.order.push(sector);
        }
        true
    }

    fn sector_count(&self) -> u64 {
        self.inner.lock().dev.sector_count()
    }
}
```

- [ ] **Step 2: Expose from `fs/src/lib.rs`**

Add after the existing `pub mod fat32; pub mod ext2;` lines:

```rust
pub mod block_cache;
pub use block_cache::CachedBlockDev;
```

- [ ] **Step 3: Build to verify no compile errors**

```bash
cargo build --manifest-path kernel/Cargo.toml --release --target x86_64-unknown-none 2>&1 | grep -E "^error|Finished"
```

Expected: `Finished` with no `error` lines.

- [ ] **Step 4: Commit**

```bash
git add fs/src/block_cache.rs fs/src/lib.rs
git commit -m "feat(fs): write-through sector block cache (CachedBlockDev)"
```

---

### Task 2: Wire `CachedBlockDev` into ext2 and FAT32 mount paths

**Files:**
- Modify: `fs/src/ext2/mod.rs`
- Modify: `fs/src/fat32/mod.rs`

- [ ] **Step 1: Wrap device in ext2 mount**

In `fs/src/ext2/mod.rs`, inside `impl FsDriver for Ext2Driver`, find the `mount` function:

```rust
fn mount(&self, dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
    let sb   = Superblock::read(dev.as_ref())?;
```

Replace with:

```rust
fn mount(&self, dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
    let dev  = Arc::new(crate::CachedBlockDev::new(dev)) as Arc<dyn BlockDev>;
    let sb   = Superblock::read(dev.as_ref())?;
```

- [ ] **Step 2: Wrap device in FAT32 mount**

In `fs/src/fat32/mod.rs`, inside `impl FsDriver for Fat32Driver`, find the `mount` function and apply the same one-line wrap at the top:

```rust
fn mount(&self, dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
    let dev = Arc::new(crate::CachedBlockDev::new(dev)) as Arc<dyn BlockDev>;
    // … rest unchanged
```

- [ ] **Step 3: Build**

```bash
cargo build --manifest-path kernel/Cargo.toml --release --target x86_64-unknown-none 2>&1 | grep -E "^error|Finished"
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Boot and verify**

```bash
make run
```

In kshell:
```
mount /dev/sda /
ls /
cat /home/hello.txt   # if previously created
```

Repeated `cat` of the same file should produce no observable difference — the cache is transparent. Serial output should show no panics.

- [ ] **Step 5: Commit**

```bash
git add fs/src/ext2/mod.rs fs/src/fat32/mod.rs
git commit -m "feat(fs): use CachedBlockDev in ext2 and FAT32 mount paths"
```

---

## Verification

- `e2fsck -n disk.img` after a QEMU session must report no errors.
- `cat` of a large file should complete — repeated reads of already-cached blocks do not re-hit VirtIO (observable as faster second cat; no functional difference visible without instrumentation).
