/*
 * ext4/mod.rs - ext4 filesystem driver (Ring-3 daemon backed)
 */

pub mod superblock;
pub mod bgdt;
pub mod inode;
pub mod extent;
pub mod dir;
pub mod bitmap_alloc;
pub mod ipc;
#[cfg(feature = "kernel")]
pub mod kernel_stub;

pub use superblock::Superblock;

#[cfg(feature = "kernel")]
extern crate alloc;
#[cfg(feature = "kernel")]
use alloc::sync::Arc;
#[cfg(feature = "kernel")]
use vfs::INode;
#[cfg(feature = "kernel")]
use crate::{BlockDev, FsDriver};

#[cfg(feature = "kernel")]
pub fn init() {
	crate::register(Arc::new(Ext4Driver));
}

#[cfg(feature = "kernel")]
struct Ext4Driver;

#[cfg(feature = "kernel")]
impl FsDriver for Ext4Driver {
	fn name(&self) -> &'static str { "ext4" }

	fn probe(&self, dev: &dyn BlockDev) -> bool {
		let sb = match Superblock::read(dev) { Some(s) => s, None => return false };
		/* Must have extents; must NOT have 64-bit (we don't support it yet) */
		use superblock::{INCOMPAT_EXTENTS, INCOMPAT_64BIT};
		sb.feature_incompat & INCOMPAT_EXTENTS != 0
			&& sb.feature_incompat & INCOMPAT_64BIT == 0
	}

	fn mount(&self, _dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
		/*
		 * The daemon is already running (spawned by kernel/src/main.rs).
		 * Create the request port if not already present, then return
		 * the root directory stub (inode 2).
		 */
		if ::ipc::IPC_GLOBAL.get_port(kernel_stub::EXT4_REQ_PORT_VAL).is_none() {
			::ipc::IPC_GLOBAL.create_port(kernel_stub::EXT4_REQ_PORT_VAL);
		}
		Some(Arc::new(kernel_stub::Ext4DirStub { ino: 2 }))
	}
}
