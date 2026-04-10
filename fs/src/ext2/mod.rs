/*
 * ext2/mod.rs - ext2 Filesystem Driver Registration
 *
 * Implements FsDriver for ext2. Call fs::ext2::init() at boot to
 * register the driver with the global filesystem registry.
 */

extern crate alloc;
use alloc::sync::Arc;
use spin::Mutex;
use vfs::INode;
use crate::{BlockDev, FsDriver};

pub mod superblock;
pub mod bgdt;
pub mod inode;
pub mod dir;
pub mod bitmap_alloc;
pub mod inode_impl;

use superblock::Superblock;
use bgdt::BgDescTable;
use inode_impl::{Ext2DirINode, Ext2State};

/* ------------------------------------------------------------------ */
/*  Ext2Driver                                                         */
/* ------------------------------------------------------------------ */

struct Ext2Driver;

impl FsDriver for Ext2Driver {
	fn name(&self) -> &'static str { "ext2" }

	/*
	 * probe - Check magic at byte 1024 (sector 2, offset 56 within sector).
	 *
	 * Superblock spans sectors 2-3. The magic field is at byte offset 56
	 * from the start of the superblock (i.e. byte 56 within sector 2).
	 */
	fn probe(&self, dev: &dyn BlockDev) -> bool {
		let mut buf = [0u8; 512];
		if !dev.read_block(2, &mut buf) { return false; }
		u16::from_le_bytes([buf[56], buf[57]]) == superblock::EXT2_MAGIC
	}

	/*
	 * mount - Parse superblock + BGDT, return root directory INode (inode 2).
	 */
	fn mount(&self, dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
		let sb   = Superblock::read(dev.as_ref())?;
		let bgdt = BgDescTable::read(dev.as_ref(), &sb);
		let state = Arc::new(Mutex::new(Ext2State { dev, sb, bgdt }));
		Some(Arc::new(Ext2DirINode { ino: 2, state }))
	}
}

/* ------------------------------------------------------------------ */
/*  Public init                                                        */
/* ------------------------------------------------------------------ */

/*
 * init - Register the ext2 driver with the global filesystem registry.
 *
 * Call once at boot before any mount operations.
 */
pub fn init() {
	crate::register(Arc::new(Ext2Driver));
}
