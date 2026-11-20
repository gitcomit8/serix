/*
 * fs/src/lib.rs - Filesystem Registry and Block Device Abstractions
 *
 * Defines the FsDriver and BlockDev traits used by all filesystem
 * submodules. Maintains a runtime registry of registered drivers,
 * modelled after Linux's register_filesystem / unregister_filesystem.
 *
 * Submodules:
 *   fat32/ - FAT32 driver (reorganised from the original fs crate)
 *   ext2/  - ext2 driver (no journal)
 *
 * Usage:
 *   fs::fat32::init();   // register FAT32 driver
 *   fs::ext2::init();    // register ext2 driver
 *   fs::probe_and_mount(dev) // try registered drivers, return root INode
 */

#![no_std]
extern crate alloc;

#[cfg(feature = "kernel")]
use alloc::sync::Arc;
#[cfg(feature = "kernel")]
use alloc::vec::Vec;
#[cfg(feature = "kernel")]
use spin::Mutex;
#[cfg(feature = "kernel")]
use vfs::INode;

#[cfg(feature = "kernel")]
pub mod block_cache;
#[cfg(feature = "kernel")]
pub mod ext2;
pub mod ext4;
#[cfg(feature = "kernel")]
pub mod gpt;
#[cfg(feature = "kernel")]
pub mod fat32;
#[cfg(any(feature = "kernel", feature = "policy-engine"))]
pub mod policy;
#[cfg(feature = "kernel")]
pub use block_cache::CachedBlockDev;

/* ------------------------------------------------------------------ */
/*  BlockDev trait                                                      */
/* ------------------------------------------------------------------ */

/*
 * trait BlockDev - Synchronous 512-byte sector I/O
 *
 * Implemented by VirtioBlockDev (below) and used by all fs drivers.
 * Passing Arc<dyn BlockDev> to mount() keeps the driver independent
 * of the underlying transport.
 */
pub trait BlockDev: Send + Sync {
	fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool;
	fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool;
	fn sector_count(&self) -> u64;
}

/* ------------------------------------------------------------------ */
/*  FsDriver trait (kernel only)                                        */
/* ------------------------------------------------------------------ */

#[cfg(feature = "kernel")]
pub trait FsDriver: Send + Sync {
	fn name(&self) -> &'static str;
	fn probe(&self, dev: &dyn BlockDev) -> bool;
	fn mount(&self, dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>>;
}

/* ------------------------------------------------------------------ */
/*  Global driver registry (kernel only)                               */
/* ------------------------------------------------------------------ */

#[cfg(feature = "kernel")]
static FS_REGISTRY: Mutex<Vec<Arc<dyn FsDriver>>> = Mutex::new(Vec::new());

#[cfg(feature = "kernel")]
pub fn register(driver: Arc<dyn FsDriver>) {
	let mut reg = FS_REGISTRY.lock();
	reg.retain(|d| d.name() != driver.name());
	reg.push(driver);
}

#[cfg(feature = "kernel")]
pub fn unregister(name: &str) {
	FS_REGISTRY.lock().retain(|d| d.name() != name);
}

#[cfg(feature = "kernel")]
pub fn probe_and_mount(dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
	let drivers: Vec<Arc<dyn FsDriver>> = FS_REGISTRY.lock().clone();
	for driver in drivers {
		if driver.probe(dev.as_ref()) {
			return driver.mount(Arc::clone(&dev));
		}
	}
	None
}

/* ------------------------------------------------------------------ */
/*  VirtioBlockDev (kernel only)                                       */
/* ------------------------------------------------------------------ */

/*
 * struct VirtioBlockDev - BlockDev over one entry of the VirtIO device table
 * @index: Device table index (0 = first disk enumerated)
 */
#[cfg(feature = "kernel")]
pub struct VirtioBlockDev {
	index: usize,
}

#[cfg(feature = "kernel")]
impl VirtioBlockDev {
	pub fn new(index: usize) -> Self {
		Self { index }
	}
}

#[cfg(feature = "kernel")]
impl BlockDev for VirtioBlockDev {
	fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
		drivers::virtio::virtio_blk_at(self.index)
			.map(|blk| blk.lock().read_sector(sector, buf).is_ok())
			.unwrap_or(false)
	}

	fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool {
		drivers::virtio::virtio_blk_at(self.index)
			.map(|blk| blk.lock().write_sector(sector, buf).is_ok())
			.unwrap_or(false)
	}

	fn sector_count(&self) -> u64 {
		drivers::virtio::virtio_blk_at(self.index)
			.map(|blk| blk.lock().capacity())
			.unwrap_or(0)
	}
}

/*
 * disk_name - Device name for a block-device index
 * @index: Device table index
 *
 * Return: "sda", "sdb", ... (allocated String, so callers can build paths)
 */
#[cfg(feature = "kernel")]
pub fn disk_name(index: usize) -> alloc::string::String {
	let mut name = alloc::string::String::from("sd");
	name.push((b'a' + (index % 26) as u8) as char);
	if index >= 26 {
		name.push((b'a' + (index / 26 - 1) as u8) as char);
	}
	name
}

/*
 * struct PartitionDev - BlockDev view of one GPT partition
 * @parent:       Whole-disk device supplying the sectors
 * @start_lba:    First sector of the partition on the parent
 * @sector_count: Length in sectors; I/O outside this range is refused
 *
 * Address translation is the whole point: a filesystem driver mounting
 * /dev/sda2 must see its superblock at sector 0.
 */
#[cfg(feature = "kernel")]
pub struct PartitionDev {
	parent: Arc<dyn BlockDev>,
	start_lba: u64,
	sector_count: u64,
}

#[cfg(feature = "kernel")]
impl PartitionDev {
	pub fn new(parent: Arc<dyn BlockDev>, start_lba: u64, sector_count: u64) -> Self {
		Self {
			parent,
			start_lba,
			sector_count,
		}
	}

	pub fn start_lba(&self) -> u64 {
		self.start_lba
	}
}

#[cfg(feature = "kernel")]
impl BlockDev for PartitionDev {
	fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
		if sector >= self.sector_count {
			return false;
		}
		self.parent.read_block(self.start_lba + sector, buf)
	}

	fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool {
		if sector >= self.sector_count {
			return false;
		}
		self.parent.write_block(self.start_lba + sector, buf)
	}

	fn sector_count(&self) -> u64 {
		self.sector_count
	}
}

/*
 * SectorReader adapter so GPT parsing works directly on a BlockDev
 */
#[cfg(feature = "kernel")]
impl gpt::SectorReader for dyn BlockDev {
	fn read_sector(&self, lba: u64, buf: &mut [u8; 512]) -> bool {
		self.read_block(lba, buf)
	}
}

/* ------------------------------------------------------------------ */
/*  BlockDevINode (kernel only)                                        */
/* ------------------------------------------------------------------ */

#[cfg(feature = "kernel")]
pub struct BlockDevINode(pub Arc<dyn BlockDev>);

#[cfg(feature = "kernel")]
impl INode for BlockDevINode {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		if buf.is_empty() {
			return 0;
		}
		let mut done = 0usize;
		let mut pos = offset;
		let mut sector_buf = [0u8; 512];

		while done < buf.len() {
			let sector = (pos / 512) as u64;
			let sec_off = pos % 512;
			if !self.0.read_block(sector, &mut sector_buf) {
				break;
			}
			let avail = 512 - sec_off;
			let copy = core::cmp::min(avail, buf.len() - done);
			buf[done..done + copy].copy_from_slice(&sector_buf[sec_off..sec_off + copy]);
			done += copy;
			pos += copy;
		}
		done
	}

	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		if buf.is_empty() {
			return 0;
		}
		let mut done = 0usize;
		let mut pos = offset;
		let mut sector_buf = [0u8; 512];

		while done < buf.len() {
			let sector = (pos / 512) as u64;
			let sec_off = pos % 512;
			/* Read-modify-write for partial sectors */
			if sec_off != 0 || buf.len() - done < 512 {
				if !self.0.read_block(sector, &mut sector_buf) {
					break;
				}
			}
			let avail = 512 - sec_off;
			let copy = core::cmp::min(avail, buf.len() - done);
			sector_buf[sec_off..sec_off + copy].copy_from_slice(&buf[done..done + copy]);
			if !self.0.write_block(sector, &sector_buf) {
				break;
			}
			done += copy;
			pos += copy;
		}
		done
	}

	fn metadata(&self) -> vfs::FileType {
		vfs::FileType::Device
	}

	fn size(&self) -> usize {
		(self.0.sector_count() * 512) as usize
	}
}

/* ------------------------------------------------------------------ */
/*  Block device registry (kernel only)                                */
/* ------------------------------------------------------------------ */

/*
 * Block device registry — maps VFS paths (e.g., "/dev/sda") to
 * Arc<dyn BlockDev>. Used by SYS_FORMAT to look up block devices
 * without needing to downcast INode trait objects.
 */
#[cfg(feature = "kernel")]
static BLOCK_DEVICES: spin::Mutex<alloc::collections::BTreeMap<alloc::string::String, Arc<dyn BlockDev>>> =
	spin::Mutex::new(alloc::collections::BTreeMap::new());

#[cfg(feature = "kernel")]
pub fn register_block_device(path: &str, dev: Arc<dyn BlockDev>) {
	let mut reg = BLOCK_DEVICES.lock();
	reg.insert(alloc::string::String::from(path), dev);
}

#[cfg(feature = "kernel")]
pub fn lookup_block_device(path: &str) -> Option<Arc<dyn BlockDev>> {
	BLOCK_DEVICES.lock().get(path).cloned()
}
