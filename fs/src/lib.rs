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
pub mod fat32;
#[cfg(feature = "kernel")]
pub mod ext2;
pub mod ext4;
#[cfg(feature = "kernel")]
pub mod block_cache;
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

#[cfg(feature = "kernel")]
pub struct VirtioBlockDev;

#[cfg(feature = "kernel")]
impl BlockDev for VirtioBlockDev {
	fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
		drivers::virtio::virtio_blk()
			.map(|blk| blk.lock().read_sector(sector, buf).is_ok())
			.unwrap_or(false)
	}

	fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool {
		drivers::virtio::virtio_blk()
			.map(|blk| blk.lock().write_sector(sector, buf).is_ok())
			.unwrap_or(false)
	}

	fn sector_count(&self) -> u64 {
		drivers::virtio::virtio_blk()
			.map(|blk| blk.lock().capacity())
			.unwrap_or(0)
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
		if buf.is_empty() { return 0; }
		let mut done = 0usize;
		let mut pos = offset;
		let mut sector_buf = [0u8; 512];

		while done < buf.len() {
			let sector = (pos / 512) as u64;
			let sec_off = pos % 512;
			if !self.0.read_block(sector, &mut sector_buf) { break; }
			let avail = 512 - sec_off;
			let copy = core::cmp::min(avail, buf.len() - done);
			buf[done..done + copy].copy_from_slice(&sector_buf[sec_off..sec_off + copy]);
			done += copy;
			pos += copy;
		}
		done
	}

	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		if buf.is_empty() { return 0; }
		let mut done = 0usize;
		let mut pos = offset;
		let mut sector_buf = [0u8; 512];

		while done < buf.len() {
			let sector = (pos / 512) as u64;
			let sec_off = pos % 512;
			/* Read-modify-write for partial sectors */
			if sec_off != 0 || buf.len() - done < 512 {
				if !self.0.read_block(sector, &mut sector_buf) { break; }
			}
			let avail = 512 - sec_off;
			let copy = core::cmp::min(avail, buf.len() - done);
			sector_buf[sec_off..sec_off + copy].copy_from_slice(&buf[done..done + copy]);
			if !self.0.write_block(sector, &sector_buf) { break; }
			done += copy;
			pos += copy;
		}
		done
	}

	fn metadata(&self) -> vfs::FileType { vfs::FileType::Device }

	fn size(&self) -> usize {
		(self.0.sector_count() * 512) as usize
	}
}
