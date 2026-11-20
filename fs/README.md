# Filesystem Drivers

## Overview

The `fs` crate implements filesystem drivers and abstractions for the Serix operating system. It currently includes:

- **FAT32**: Full read/write driver with LFN support, FAT cluster chain management
- **ext2**: Basic parser and stub support
- **ext4**: Ring-3 daemon backed (`ext4d`) with HTree indexing and JBD2 journaling
- **GPT**: Validated GUID Partition Table parsing and offset partition devices
- **Block devices**: `VirtioBlockDev` (one indexed disk) and `PartitionDev` (a GPT partition as its own `BlockDev`)

## Crate Structure

```
fs/
├── lib.rs        # BlockDev trait, FsDriver registry, probe_and_mount, VirtioBlockDev, PartitionDev
├── gpt.rs        # GPT parsing and validation (host-testable, no kernel deps)
├── fat32/        # FAT32 driver (BPB parsing, FAT table, directory entries)
├── ext2/         # ext2 driver (superblock, inode, directory)
├── ext4/         # ext4 support
│   ├── mod.rs    # Driver registration, probe/mount, kernel stub
│   ├── superblock.rs # ext4 superblock parser
│   ├── inode.rs  # Inode read/write with extent tree support
│   ├── extent.rs # Extent tree block resolution (inline + multi-level)
│   ├── dir.rs    # Directory entry parser (lookup, readdir, mkdir, unlink, rmdir)
│   ├── htree.rs  # HTree directory indexing (hash, tree traversal, index creation)
│   ├── journal.rs # JBD2 journal (superblock, transactions, recovery replay)
│   ├── ipc.rs    # IPC protocol constants (message IDs, port numbers)
│   ├── format.rs # Disk formatting helpers
│   └── kernel_stub.rs # VFS INode stubs that proxy to ext4d daemon
├── ext4d/        # Ring-3 ext4 filesystem daemon (separate crate)
├── policy/       # Policy engine for access control
└── block_cache.rs # LRU-ish block device caching
```

## Filesystem Registry

The kernel registers filesystem drivers at boot and uses `probe_and_mount()` to detect and mount a filesystem on a block device:

```rust
// Register drivers
fs::fat32::init();
fs::ext2::init();
fs::ext4::init();

// Probe and auto-mount (after ext4d is spawned)
if let Some(root_ino) = fs::probe_and_mount(Arc::new(fs::VirtioBlockDev::new(0))) {
    vfs::mount("/mnt", root_ino);
}
```

`probe_and_mount()` iterates registered drivers, calls each driver's `probe()` to detect the filesystem type, then calls `mount()` to return the root INode.

## Block Devices and Partitions

`fs::BlockDev` is the only interface filesystem drivers see; it is implemented by:

- **`VirtioBlockDev::new(index)`** — one entry of the driver's block-device table (index 0 is `/dev/sda`).
- **`PartitionDev::new(parent, start_lba, sector_count)`** — an offset view of a parent device. Sector 0 of the partition is `start_lba` on the parent, and any I/O at or past `sector_count` is refused, so a filesystem driver mounting `/dev/sda2` cannot escape its partition.

### GPT parsing (`gpt.rs`)

`read_gpt(&dev, disk_sectors)` parses the **primary** GPT header at LBA 1 and returns `Vec<Partition>` or a `GptError`. Validation happens in this order, and the first failure rejects the whole table — no partial results are returned:

1. `EFI PART` signature → `NoGpt`
2. Revision is 1.0 → `BadRevision`
3. Header size in `92..=512` → `BadHeaderSize`
4. Header CRC32 (CRC field zeroed) → `BadHeaderCrc`
5. Entry count/size bounds and array size cap → `BadEntryArray`
6. Entry-array CRC32 over exactly `entry_count * entry_size` bytes → `BadEntryArrayCrc`
7. Any unreadable sector → `Io`

Collected entries skip all-zero type GUIDs and zero-length or inverted ranges; entries starting past the disk are dropped and entries extending past it are clipped to the last addressable sector. Partition numbers are assigned contiguously in table order, which drives `/dev/sdXN` naming.

The module depends only on `alloc`, so its 11 unit tests run on the host. The header comment of `fs/src/gpt.rs` contains the harness commands; the kernel build does not run them.

**Not implemented:** backup-GPT fallback, overlap detection, protective-MBR interpretation, and non-512-byte logical sectors.

## ext4 Architecture

### Ring-3 Daemon Model

Unlike FAT32 (which runs entirely in Ring 0), ext4 uses a **Ring-3 daemon** model. The kernel maintains only IPC stubs that forward VFS operations to `ext4d` via synchronous IPC:

```
kshell/other Ring-0 task
        |
        | VFS syscall (open, read, write, mkdir, etc.)
        |
   Kernel ext4 stub (kernel_stub.rs)
        |
        | IPC: SYS_SEND → EXT4_REQ_PORT (0x4E00)
        |
   ext4d daemon (Ring 3)
        |
        | read/write_block() on /dev/sda (or /dev/sdXN via PartitionDev)
        |
   VirtIO block device
```

The daemon parses the ext4 on-disk format (superblock, BGDT, inodes, extent trees) and serves operations via IPC. This keeps the complex ext4 implementation isolated from the kernel.

### HTree Directory Indexing

ext4 directories can use a **hash tree** (HTree) for O(log n) lookups instead of linear scanning:

- **Hash function**: ext4 `hash_name_2()` — Jenkins one-at-a-time with character-dependent bit rotation
- **dx_root**: Header in the first directory block containing the hash version, entry count, and pointer to leaf block
- **dx_node**: Multi-level index nodes for large directories
- **Leaf blocks**: Contain the actual directory entries (name, inode number)

Index creation (`create_htree_index()`):
1. Allocates a dx_root header in the first data block
2. Allocates a separate leaf block for "." and ".." entries
3. Updates the inode's extent tree (2 extent entries: index block + leaf block)

Automatic conversion (`maybe_convert_to_htree()`):
- Triggers when directory entry count ≥ 3
- Called after `add_entry()` in directory operations

### JBD2 Journal

The ext4 driver includes a **JBD2 (Journaling Block Device 2)** implementation in ordered mode:

- **Journal superblock**: Magic number, blocksize, head/tail pointers, sequence numbers
- **Transactions**: Begin → buffer metadata → commit (write header + buffers + commit record)
- **Crash recovery**: Scans journal for valid transaction sequences, replays committed blocks to original locations
- **Journal integration**: Every mutating operation (write, mkdir, create, unlink, rmdir) begins a transaction and commits on completion

Journal layout on disk:
```
Journal blocks (64 blocks, 256 KB with 4 KB block size)
┌─────────────────┐
│ Journal Superblock │
├─────────────────┤
│ Transaction N    │
│   Header         │
│   Block buffers  │
│   Commit record  │
├─────────────────┤
│ Transaction N+1  │
│   ...            │
└─────────────────┘
```

## FAT32 Driver

[Original FAT32 documentation continues below...]

The driver supports:

- BPB parsing and filesystem layout discovery
- FAT cluster chain traversal and allocation
- Directory entry reading (8.3 short names and Long File Names)
- Directory entry creation (LFN + 8.3 pair)
- File read/write with arbitrary byte offsets

## Architecture

### Design Philosophy

The driver is structured as a set of layered internal functions with a thin VFS adapter on top. There is no async I/O -- all operations are synchronous and block on VirtIO completion. Global filesystem state is held in a single `Once<Mutex<Fat32State>>` following the Serix global state pattern.

### Component Layers

```
VFS INode Interface (FatDirINode, FatFileINode)
         |
   Path Resolution (find_entry_in_dir)
         |
   Directory Parsing (read_dir_entries, create_dir_entry)
         |
   FAT Operations (fat_read_entry, fat_write_entry, fat_alloc_cluster)
         |
   Raw I/O (read_sector, write_sector)
         |
   VirtIO Block Device (drivers crate)
```

### Initialization Flow

1. Caller invokes `fs::mount()` after VirtIO queues are initialized
2. Sector 0 is read from the block device
3. The BPB is parsed and validated (boot signature `0x55AA`, non-zero fields)
4. On success, the global `FAT32` state is initialized with the parsed BPB
5. `FatDirINode::root()` can then be used as the root of the FAT32 tree

```rust
/* Mount the FAT32 filesystem */
if fs::mount() {
	let root = fs::FatDirINode::root();
	/* root implements vfs::INode */
}
```

## On-Disk Format

The driver expects a standard FAT32 volume, typically created with `mkfs.vfat -F 32`. The disk layout is:

```
Sector 0          BPB / Boot Sector (512 bytes)
                  +-------------------------------+
                  | Jump boot code (3 bytes)      |
                  | OEM name (8 bytes)            |
                  | Bytes per sector [11-12]      |
                  | Sectors per cluster [13]      |
                  | Reserved sectors [14-15]      |
                  | Number of FATs [16]           |
                  | ...                           |
                  | FAT32: sectors/FAT [36-39]    |
                  | FAT32: root cluster [44-47]   |
                  | Boot signature 0x55AA [510]   |
                  +-------------------------------+

Reserved Region   Sectors 0 .. reserved_sectors-1
                  (includes BPB, FSInfo, backup boot sector)

FAT Region        reserved_sectors .. reserved_sectors + (fat_count * sectors_per_fat) - 1
                  Each FAT entry is 4 bytes (28 bits used).
                  Entry 0/1: reserved.  Entry N >= 2: next cluster or EOC.

Data Region       First sector of cluster 2.
                  Cluster N maps to sector:
                    data_start + (N - 2) * sectors_per_cluster
```

### FAT Entry Values

| Value              | Meaning                              |
| ------------------ | ------------------------------------ |
| `0x00000000`       | Free cluster                         |
| `0x00000002`-`max` | Next cluster in chain                |
| `>= 0x0FFFFFF8`    | End of cluster chain (EOC)           |

The upper 4 bits of each 32-bit FAT entry are reserved and preserved on write.

### Cluster Chains

Files and directories are stored as linked lists of clusters. The FAT table maps each cluster number to the next cluster in the chain, or to an EOC marker. The `cluster_chain()` function collects all clusters for a given file or directory by walking this linked list:

```rust
/* Collect the full cluster chain starting at first_cluster */
fn cluster_chain(bpb: &Bpb, first_cluster: u32) -> Vec<u32> {
	let mut chain = Vec::new();
	let mut cur = first_cluster;
	while cur >= 2 && cur < FAT32_EOC {
		chain.push(cur);
		let next = fat_read_entry(bpb, cur);
		if next >= FAT32_EOC || next < 2 {
			break;
		}
		cur = next;
	}
	chain
}
```

## API Reference

### Public Functions

#### `mount() -> bool`

Reads sector 0, parses the BPB, and initializes the global FAT32 state. Must be called after VirtIO block device queues are set up (`setup_queues_global()`). Returns `true` on success, `false` if no VirtIO device is present or BPB parsing fails.

### Public Types

#### `FatDirINode`

Represents a directory on the FAT32 filesystem. Implements `vfs::INode`.

```rust
pub struct FatDirINode {
	cluster: u32, /* First cluster of the directory */
}
```

**Construction:**

- `FatDirINode::root()` -- Returns the root directory INode (uses `root_cluster` from BPB)
- `FatDirINode::new(cluster)` -- Wraps an arbitrary directory cluster

**VFS INode methods:**

| Method     | Behavior                                                              |
| ---------- | --------------------------------------------------------------------- |
| `read()`   | Returns 0 (directories are not readable as byte streams)              |
| `write()`  | Returns 0 (directories are not writable as byte streams)              |
| `metadata()` | Returns `FileType::Directory`                                       |
| `lookup(name)` | Searches directory entries for `name` (case-insensitive). Returns `FatDirINode` for directories, `FatFileINode` for files. |
| `insert(name, node)` | Creates a new file entry: allocates a cluster, writes LFN + 8.3 directory entries. The `node` parameter is currently ignored; a fresh file is always created. |

#### `FatFileINode`

Represents a file on the FAT32 filesystem. Implements `vfs::INode`.

```rust
pub struct FatFileINode {
	first_cluster: u32,      /* First data cluster               */
	size: Mutex<u32>,        /* Current file size (updated on write) */
	entry_sector: u64,       /* Sector of the 8.3 directory entry */
	entry_offset: usize,     /* Byte offset within that sector    */
}
```

**VFS INode methods:**

| Method     | Behavior                                                              |
| ---------- | --------------------------------------------------------------------- |
| `read(offset, buf)` | Reads up to `buf.len()` bytes starting at `offset`. Returns bytes read. Stops at file size. |
| `write(offset, buf)` | Writes `buf` at `offset`. Extends the cluster chain and updates the on-disk directory entry size if writing past EOF. Returns bytes written. |
| `metadata()` | Returns `FileType::File`                                            |
| `size()`   | Returns current file size in bytes                                    |

### Usage Example

```rust
use vfs::INode;

/* After mount(), look up a file and read its contents */
let root = fs::FatDirINode::root();
if let Some(file_node) = root.lookup("hello.txt") {
	let mut buf = [0u8; 256];
	let n = file_node.read(0, &mut buf);
	/* buf[..n] contains the file data */
}

/* Create a new file and write to it */
let dummy: alloc::sync::Arc<dyn INode> =
	alloc::sync::Arc::new(fs::FatDirINode::new(0));
root.insert("output.txt", dummy).expect("insert failed");
if let Some(file_node) = root.lookup("output.txt") {
	file_node.write(0, b"Hello from Serix\n");
}
```

## Directory Entry Format

### Short File Name (8.3 / SFN)

Each SFN directory entry is 32 bytes:

```
Offset  Size  Field
------  ----  -----
0       8     File name (space-padded, uppercase)
8       3     Extension (space-padded, uppercase)
11      1     Attributes (RDONLY|HIDDEN|SYSTEM|VOLID|DIR|ARCHIVE)
12      1     Reserved (NT case flags)
13      1     Creation time (tenths of second)
14      2     Creation time
16      2     Creation date
18      2     Last access date
20      2     First cluster high word
22      2     Last write time
24      2     Last write date
26      2     First cluster low word
28      4     File size in bytes
```

Special first-byte values:

- `0x00` -- End of directory (no more entries follow)
- `0xE5` -- Deleted entry (slot is free for reuse)

### Long File Name (LFN)

LFN entries use attribute byte `0x0F` (`RDONLY|HIDDEN|SYSTEM|VOLID`) and store up to 13 UCS-2 characters each. They appear in reverse sequence order immediately before the corresponding 8.3 entry:

```
LFN Entry (32 bytes):
Offset  Size  Field
------  ----  -----
0       1     Sequence number (bit 6 set = last LFN entry)
1       10    Characters 1-5 (UCS-2, 2 bytes each)
11      1     Attributes (always 0x0F)
12      1     Type (always 0x00)
13      1     Checksum of 8.3 name
14      12    Characters 6-11 (UCS-2)
26      2     First cluster (always 0x0000)
28      4     Characters 12-13 (UCS-2)
```

The driver creates an LFN + SFN pair for every new file via `create_dir_entry()`. The checksum in each LFN entry is computed from the 8.3 short name using a rotating sum algorithm.

## Linux Interop

The `disk.img` file used by QEMU is a raw FAT32 image. You can mount it on a Linux host for inspection or pre-population:

```bash
/* Mount the disk image */
sudo mount -o loop disk.img /mnt

/* Inspect contents */
ls -la /mnt/

/* Copy files in */
sudo cp myfile.txt /mnt/

/* Unmount */
sudo umount /mnt
```

The image is created by the Makefile with `mkfs.vfat -F 32`.

## Dependencies

### Internal Crates

- **drivers**: VirtIO block device interface (`virtio_blk_at()`, `device_count()`, `read_sector()`, `write_sector()`)
- **vfs**: `INode` trait and `FileType` enum that this crate implements
- **hal**: Serial console output for debug/error messages (`serial_println!`)

### External Crates

- **spin** (0.10.0): `Mutex` for interior mutability of file size, `Once` for one-time global initialization
- **alloc**: `String`, `Vec`, `Arc` for dynamic data structures (via kernel heap)

## Limitations / Known Issues

1. **No duplicate file check on insert**: The `insert()` method does not check whether a file with the same name already exists. Re-running the kernel will create duplicate directory entries for the same filename.

2. **No delete/unlink support**: There is no way to remove files or directory entries. Deleted entries (`0xE5`) from external tools are recognized as free slots, but the driver cannot mark entries as deleted.

3. **No subdirectory creation**: The `insert()` method only creates files (with `ATTR_ARCHIVE`). Creating subdirectories would require allocating a cluster and writing `.` and `..` entries.

4. **Single-sector clusters only tested**: The driver is written to handle multi-sector clusters, but has only been tested with `sectors_per_cluster=1` (the default for small `mkfs.vfat` images).

5. **DMA pages leaked**: The VirtIO block driver allocates DMA pages for each sector I/O operation. These pages are not freed after the transfer completes, resulting in a slow memory leak over time.

6. **No timestamp support**: File creation, modification, and access timestamps are always written as zero.

7. **Case-insensitive lookup only**: `lookup()` uses `eq_ignore_ascii_case`, matching FAT32 semantics, but `insert()` stores names as-is (uppercased only in the SFN portion).

8. **No FSInfo sector updates**: The FAT32 FSInfo sector (free cluster count, next free cluster hint) is not read or updated, which may cause `fsck` warnings on Linux.

## License

GPL-3.0 (see LICENSE file in repository root)

## References

- [Microsoft FAT32 File System Specification](https://download.microsoft.com/download/1/6/1/161ba512-40e2-4cc9-843a-923143f3456c/fatgen103.doc)
- [OSDev Wiki: FAT](https://wiki.osdev.org/FAT)
- [Serix VFS Crate](../vfs/README.md)
- [Serix VirtIO Driver](../drivers/README.md)
