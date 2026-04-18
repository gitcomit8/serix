/*
 * ext4/mod.rs - ext4 filesystem driver (Ring-3 daemon backed)
 */

extern crate alloc;
use alloc::sync::Arc;
use vfs::INode;
use crate::{BlockDev, FsDriver};

pub mod superblock;
pub mod bgdt;
pub mod inode;

pub use superblock::Superblock;

pub fn init() {
	crate::register(Arc::new(Ext4Driver));
}

struct Ext4Driver;

impl FsDriver for Ext4Driver {
	fn name(&self) -> &'static str { "ext4" }

	fn probe(&self, dev: &dyn BlockDev) -> bool {
		let sb = match Superblock::read(dev) { Some(s) => s, None => return false };
		/* Must have extents; must NOT have 64-bit (we don't support it yet) */
		use superblock::{INCOMPAT_EXTENTS, INCOMPAT_64BIT};
		sb.feature_incompat & INCOMPAT_EXTENTS != 0
			&& sb.feature_incompat & INCOMPAT_64BIT == 0
	}

	fn mount(&self, dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
		/* Implemented in Task 8 after daemon is ready */
		let _ = dev;
		None
	}
}
