/*
 * ext2/inode_impl.rs - VFS INode Implementations for ext2
 *
 * Borrow-checker note: when a method needs both &dyn BlockDev and
 * &mut Superblock/&mut BgDescTable simultaneously, we:
 *   1) Arc::clone the device so it's independent of the lock guard.
 *   2) Use `let e = &mut *st;` to get an explicit &mut Ext2State, which
 *      allows Rust to split disjoint field borrows (&mut e.sb, &mut e.bgdt).
 */

extern crate alloc;
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use vfs::{FileType, INode};
use crate::BlockDev;
use super::superblock::Superblock;
use super::bgdt::BgDescTable;
use super::inode::Inode;
use super::{bitmap_alloc as ext2_alloc, dir};

/* ------------------------------------------------------------------ */
/*  Shared filesystem state                                            */
/* ------------------------------------------------------------------ */

pub struct Ext2State {
	pub dev:  Arc<dyn BlockDev>,
	pub sb:   Superblock,
	pub bgdt: BgDescTable,
}

/* ------------------------------------------------------------------ */
/*  Block I/O helpers                                                  */
/* ------------------------------------------------------------------ */

fn read_block_bytes(dev: &dyn BlockDev, sb: &Superblock, blk: u32) -> Vec<u8> {
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(blk as u64);
	let bsz = sb.block_size();
	let mut out = Vec::with_capacity(bsz);
	for s in 0..spb {
		let mut buf = [0u8; 512];
		dev.read_block(sec + s, &mut buf);
		out.extend_from_slice(&buf);
	}
	out
}

fn write_block_bytes(dev: &dyn BlockDev, sb: &Superblock, blk: u32, data: &[u8]) {
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(blk as u64);
	for s in 0..spb as usize {
		let mut buf = [0u8; 512];
		buf.copy_from_slice(&data[s * 512..(s + 1) * 512]);
		dev.write_block(sec + s as u64, &buf);
	}
}

/* ------------------------------------------------------------------ */
/*  Ext2FileINode                                                      */
/* ------------------------------------------------------------------ */

pub struct Ext2FileINode {
	pub ino:   u32,
	pub state: Arc<Mutex<Ext2State>>,
}

impl INode for Ext2FileINode {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		if buf.is_empty() { return 0; }
		let st  = self.state.lock();
		let dev = Arc::clone(&st.dev);
		let inode = match Inode::read(dev.as_ref(), &st.sb, &st.bgdt, self.ino) {
			Some(i) => i, None => return 0,
		};
		let file_size = inode.size();
		if offset >= file_size { return 0; }

		let bsz      = st.sb.block_size();
		let to_read  = core::cmp::min(buf.len(), file_size - offset);
		let mut done = 0usize;

		while done < to_read {
			let pos     = offset + done;
			let blk_idx = (pos / bsz) as u32;
			let blk_off = pos % bsz;
			let phys = match inode.get_block(dev.as_ref(), &st.sb, blk_idx) {
				Some(p) => p, None => break,
			};
			let block = read_block_bytes(dev.as_ref(), &st.sb, phys);
			let avail = core::cmp::min(bsz - blk_off, to_read - done);
			buf[done..done + avail].copy_from_slice(&block[blk_off..blk_off + avail]);
			done += avail;
		}
		done
	}

	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		if buf.is_empty() { return 0; }
		let mut st  = self.state.lock();
		let dev     = Arc::clone(&st.dev);
		let e       = &mut *st;
		let mut inode = match Inode::read(dev.as_ref(), &e.sb, &e.bgdt, self.ino) {
			Some(i) => i, None => return 0,
		};
		let bsz      = e.sb.block_size();
		let mut done = 0usize;

		while done < buf.len() {
			let pos     = offset + done;
			let blk_idx = (pos / bsz) as u32;
			let blk_off = pos % bsz;

			let phys = match inode.get_block(dev.as_ref(), &e.sb, blk_idx) {
				Some(p) => p,
				None => {
					let p = match ext2_alloc::alloc_block(dev.as_ref(), &mut e.sb, &mut e.bgdt) {
						Some(p) => p, None => break,
					};
					let zeros = alloc::vec![0u8; bsz];
					write_block_bytes(dev.as_ref(), &e.sb, p, &zeros);
					inode.set_block(dev.as_ref(), &mut e.sb, &mut e.bgdt, blk_idx, p);
					inode.blocks += e.sb.sectors_per_block() as u32;
					p
				}
			};

			let avail = core::cmp::min(bsz - blk_off, buf.len() - done);
			let mut block = read_block_bytes(dev.as_ref(), &e.sb, phys);
			block[blk_off..blk_off + avail].copy_from_slice(&buf[done..done + avail]);
			write_block_bytes(dev.as_ref(), &e.sb, phys, &block);
			done += avail;
		}

		let new_end = offset + done;
		if new_end > inode.size() { inode.size = new_end as u32; }
		inode.write(dev.as_ref(), &e.sb, &e.bgdt);
		done
	}

	fn metadata(&self) -> FileType { FileType::File }

	fn size(&self) -> usize {
		let st  = self.state.lock();
		let dev = Arc::clone(&st.dev);
		Inode::read(dev.as_ref(), &st.sb, &st.bgdt, self.ino)
			.map(|i| i.size())
			.unwrap_or(0)
	}
}

/* ------------------------------------------------------------------ */
/*  Ext2DirINode                                                       */
/* ------------------------------------------------------------------ */

pub struct Ext2DirINode {
	pub ino:   u32,
	pub state: Arc<Mutex<Ext2State>>,
}

impl INode for Ext2DirINode {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }
	fn write(&self, _offset: usize, _buf: &[u8]) -> usize { 0 }
	fn metadata(&self) -> FileType { FileType::Directory }

	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>> {
		let st  = self.state.lock();
		let dev = Arc::clone(&st.dev);
		let dir_inode = Inode::read(dev.as_ref(), &st.sb, &st.bgdt, self.ino)?;
		let child_ino = dir::lookup_in_dir(dev.as_ref(), &st.sb, &st.bgdt, &dir_inode, name)?;
		let child     = Inode::read(dev.as_ref(), &st.sb, &st.bgdt, child_ino)?;
		let state = Arc::clone(&self.state);
		if child.is_dir() {
			Some(Arc::new(Ext2DirINode  { ino: child_ino, state }))
		} else {
			Some(Arc::new(Ext2FileINode { ino: child_ino, state }))
		}
	}

	fn readdir(&self) -> Option<Vec<(String, FileType)>> {
		let st  = self.state.lock();
		let dev = Arc::clone(&st.dev);
		let dir_inode = Inode::read(dev.as_ref(), &st.sb, &st.bgdt, self.ino)?;
		Some(dir::readdir(dev.as_ref(), &st.sb, &st.bgdt, &dir_inode))
	}

	fn mkdir(&self, name: &str) -> Result<(), &'static str> {
		let mut st  = self.state.lock();
		let dev     = Arc::clone(&st.dev);
		let e       = &mut *st;

		let child_ino = ext2_alloc::alloc_inode(dev.as_ref(), &mut e.sb, &mut e.bgdt)
			.ok_or("no free inodes")?;
		let blk = ext2_alloc::alloc_block(dev.as_ref(), &mut e.sb, &mut e.bgdt)
			.ok_or("no free blocks")?;

		let bsz = e.sb.block_size();
		let spb = e.sb.sectors_per_block();

		let mut dir_blk = alloc::vec![0u8; bsz];
		/* . entry */
		dir_blk[0..4].copy_from_slice(&child_ino.to_le_bytes());
		dir_blk[4..6].copy_from_slice(&12u16.to_le_bytes());
		dir_blk[6] = 1;
		dir_blk[7] = dir::EXT2_FT_DIR;
		dir_blk[8] = b'.';
		/* .. entry */
		let rec_rem = (bsz - 12) as u16;
		dir_blk[12..16].copy_from_slice(&self.ino.to_le_bytes());
		dir_blk[16..18].copy_from_slice(&rec_rem.to_le_bytes());
		dir_blk[18] = 2;
		dir_blk[19] = dir::EXT2_FT_DIR;
		dir_blk[20] = b'.';
		dir_blk[21] = b'.';

		let sec = e.sb.block_to_sector(blk as u64);
		for s in 0..spb as usize {
			let mut buf = [0u8; 512];
			buf.copy_from_slice(&dir_blk[s * 512..(s + 1) * 512]);
			dev.write_block(sec + s as u64, &buf);
		}

		let mut child = Inode {
			mode:   super::inode::EXT2_S_IFDIR | 0o755,
			size:   bsz as u32,
			blocks: spb as u32,
			block:  [0u32; 15],
			ino:    child_ino,
		};
		child.block[0] = blk;
		child.write(dev.as_ref(), &e.sb, &e.bgdt);

		let mut parent_inode = Inode::read(dev.as_ref(), &e.sb, &e.bgdt, self.ino)
			.ok_or("parent inode read failed")?;
		dir::add_entry(
			dev.as_ref(), &mut e.sb, &mut e.bgdt,
			&mut parent_inode, name, child_ino, dir::EXT2_FT_DIR,
		);

		let g = e.sb.inode_block_group(self.ino) as usize;
		e.bgdt.get_mut(g).used_dirs += 1;
		e.bgdt.write_entry(dev.as_ref(), &e.sb, g);

		Ok(())
	}

	fn create_file(&self, name: &str) -> Result<Arc<dyn INode>, &'static str> {
		let mut st  = self.state.lock();
		let dev     = Arc::clone(&st.dev);
		let e       = &mut *st;

		let child_ino = ext2_alloc::alloc_inode(dev.as_ref(), &mut e.sb, &mut e.bgdt)
			.ok_or("no free inodes")?;

		let child = Inode {
			mode:   super::inode::EXT2_S_IFREG | 0o644,
			size:   0,
			blocks: 0,
			block:  [0u32; 15],
			ino:    child_ino,
		};
		child.write(dev.as_ref(), &e.sb, &e.bgdt);

		let mut parent_inode = Inode::read(dev.as_ref(), &e.sb, &e.bgdt, self.ino)
			.ok_or("parent inode read failed")?;
		dir::add_entry(
			dev.as_ref(), &mut e.sb, &mut e.bgdt,
			&mut parent_inode, name, child_ino, dir::EXT2_FT_REG_FILE,
		);

		let state = Arc::clone(&self.state);
		Ok(Arc::new(Ext2FileINode { ino: child_ino, state }))
	}

	fn insert(&self, _name: &str, _node: Arc<dyn INode>) -> Result<(), &'static str> {
		Err("use create_file or mkdir for ext2 directories")
	}

	fn unlink(&self, name: &str) -> Result<(), &'static str> {
		let mut st  = self.state.lock();
		let dev     = Arc::clone(&st.dev);
		let e       = &mut *st;

		let dir_inode = Inode::read(dev.as_ref(), &e.sb, &e.bgdt, self.ino)
			.ok_or("dir inode read failed")?;
		let child_ino = dir::lookup_in_dir(dev.as_ref(), &e.sb, &e.bgdt, &dir_inode, name)
			.ok_or("not found")?;
		let child_inode = Inode::read(dev.as_ref(), &e.sb, &e.bgdt, child_ino)
			.ok_or("child inode read failed")?;

		let bsz      = e.sb.block_size();
		let n_blocks = (child_inode.size() + bsz - 1) / bsz;
		for b in 0..n_blocks as u32 {
			if let Some(phys) = child_inode.get_block(dev.as_ref(), &e.sb, b) {
				ext2_alloc::free_block(dev.as_ref(), &mut e.sb, &mut e.bgdt, phys);
			}
		}
		ext2_alloc::free_inode(dev.as_ref(), &mut e.sb, &mut e.bgdt, child_ino);

		if !dir::remove_entry(dev.as_ref(), &e.sb, &e.bgdt, &dir_inode, name) {
			return Err("entry not found");
		}
		Ok(())
	}
}
