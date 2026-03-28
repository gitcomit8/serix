# VFS Module

## Overview

The VFS (Virtual File System) module provides an abstract filesystem layer for the Serix kernel. It defines the `INode` trait as the common interface for files, directories, and devices, along with in-memory implementations (`RamFile`, `RamDir`) and a global root with path resolution. All filesystem nodes use `Arc<dyn INode>` for shared ownership and are protected by spinlock-based mutexes for concurrent access.

## Architecture

### INode Trait

The `INode` trait is the central abstraction for all filesystem nodes. Every node type (file, directory, device) implements this trait. Methods with default implementations allow file-like nodes to omit directory-specific operations and vice versa.

```rust
pub trait INode: Send + Sync {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize;
	fn write(&self, offset: usize, buf: &[u8]) -> usize;
	fn metadata(&self) -> FileType;
	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>>;   // default: None
	fn insert(&self, name: &str, node: Arc<dyn INode>)
		-> Result<(), &'static str>;                       // default: Err("Not a directory")
	fn size(&self) -> usize;                                   // default: 0
}
```

| Method     | Description                                          | Default        |
|------------|------------------------------------------------------|----------------|
| `read`     | Read data from the node starting at `offset`         | Required       |
| `write`    | Write data to the node starting at `offset`          | Required       |
| `metadata` | Return the `FileType` of this node                   | Required       |
| `lookup`   | Look up a child node by name (directories only)      | `None`         |
| `insert`   | Insert a child node by name (directories only)       | `Err(...)` |
| `size`     | Return the size of the node in bytes                  | `0`            |

### FileType Enum

```rust
pub enum FileType {
	File,
	Directory,
	Device,
}
```

Classifies nodes into three categories. The `Device` variant is used by driver subsystems (e.g., `BlockDevice` in the `drivers` crate) to expose hardware through the VFS.

## Implementations

### RamFile

An in-memory file backed by a `Mutex<Vec<u8>>`. Supports byte-level reads and writes at arbitrary offsets. Writes beyond the current length automatically extend the buffer with zero padding.

```rust
let file = RamFile::new("hello.txt");
file.write(0, b"Hello, world!");
let mut buf = [0u8; 13];
file.read(0, &mut buf);
```

**Key behaviors**:

- `read` returns 0 if `offset` is past end-of-file
- `write` resizes the backing vector as needed via `Vec::resize`
- `metadata` returns `FileType::File`
- `size` returns the current length of the backing vector

### RamDir

An in-memory directory backed by a `Mutex<Vec<(String, Arc<dyn INode>)>>`. Children are stored as name-node pairs in insertion order.

```rust
let root = RamDir::new("/");
let file = Arc::new(RamFile::new("hello.txt"));
root.insert("hello.txt", file).unwrap();
let found = root.lookup("hello.txt"); // Some(...)
```

**Key behaviors**:

- `read` and `write` are no-ops (return 0)
- `metadata` returns `FileType::Directory`
- `lookup` performs a linear scan over children by name
- `insert` rejects duplicate names with `Err("File exists")`

## Global Root

The VFS provides a global root inode and path resolution through two functions:

### set_root

```rust
pub fn set_root(root: Arc<dyn INode>);
```

Sets the global VFS root inode. Uses `spin::Once` to ensure the root is set exactly once. Subsequent calls are silently ignored.

### lookup_path

```rust
pub fn lookup_path(path: &str) -> Option<Arc<dyn INode>>;
```

Resolves an absolute path to an inode by walking the directory tree from the global root.

**Path Resolution Algorithm**:

1. Retrieve the global root via `VFS_ROOT.get()`. Return `None` if no root has been set.
2. If the path is `"/"` or empty, return the root inode.
3. Strip the leading `/` and split the path by `'/'`.
4. For each non-empty path component, call `INode::lookup` on the current node.
5. If any lookup returns `None`, the entire resolution fails.
6. Return the final resolved inode.

**Examples**:

```rust
// After setting root and populating directories:
let root = lookup_path("/");              // Returns the root RamDir
let file = lookup_path("/hello.txt");     // Single-level lookup
let deep = lookup_path("/dev/vda");       // Multi-level lookup
let none = lookup_path("/nonexistent");   // Returns None
```

## Dependencies

### External Crates

- **spin** (0.10.0): Spinlock-based `Mutex` and `Once` for interrupt-safe synchronization
- **alloc**: `String`, `Vec`, `Arc` from the Rust `alloc` crate (`no_std` compatible)

## Usage

The VFS is typically initialized during kernel boot after the heap is available:

```rust
use alloc::sync::Arc;
use vfs::{RamDir, RamFile, set_root, lookup_path};

// Create root directory and populate it
let root = Arc::new(RamDir::new("/"));
let file = Arc::new(RamFile::new("hello.txt"));
file.write(0, b"Hello from Serix!");
root.insert("hello.txt", file).unwrap();

// Set as global root
set_root(root);

// Resolve paths anywhere in the kernel
if let Some(node) = lookup_path("/hello.txt") {
	let mut buf = [0u8; 64];
	let n = node.read(0, &mut buf);
	// buf[..n] contains "Hello from Serix!"
}
```

Other crates (e.g., `drivers`) implement `INode` for their own node types (such as `BlockDevice`) to expose hardware through the same VFS interface.

## License

GPL-3.0 (see LICENSE file in repository root)
