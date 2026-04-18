/*
 * ext4/inode.rs - ext4 Inode Read/Write
 *
 * ext4 inodes are typically 256 bytes (inode_size field in superblock).
 * We parse the fields we care about and leave extended attributes untouched
 * via read-modify-write.
 *
 * Key difference from ext2: the block[] array at offset 40 is an extent
 * tree root when EXT4_INODE_EXTENTS_FL (0x80000) is set in i_flags.
 */

extern crate alloc;
use alloc::vec::Vec;
use crate::BlockDev;
use super::superblock::Superblock;
use super::bgdt::BgDescTable;

pub const EXT4_S_IFREG: u16 = 0x8000;
pub const EXT4_S_IFDIR: u16 = 0x4000;
pub const EXT4_EXTENTS_FL: u32 = 0x0008_0000;

pub struct Inode {
	pub mode:		u16,
	pub links_count:	u16,
	pub size:		u32,
	pub blocks:		u32,   /* 512-byte units */
	pub flags:		u32,
	pub block:		[u32; 15],  /* extent tree root or direct/indirect */
	pub ino:		u32,
}

impl Inode {
	pub fn is_dir(&self)		-> bool { self.mode & 0xF000 == EXT4_S_IFDIR }
	pub fn is_file(&self)		-> bool { self.mode & 0xF000 == EXT4_S_IFREG }
	pub fn uses_extents(&self)	-> bool { self.flags & EXT4_EXTENTS_FL != 0 }
	pub fn size(&self)		-> usize { self.size as usize }

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

	pub fn read(dev: &dyn BlockDev, sb: &Superblock, bgdt: &BgDescTable, ino: u32) -> Option<Self> {
		let group  = sb.inode_block_group(ino) as usize;
		let idx    = sb.inode_local_index(ino) as usize;
		let isz    = sb.inode_size as usize;
		let bsz    = sb.block_size();
		let bg     = bgdt.get(group);
		let byte_off   = idx * isz;
		let block_idx  = byte_off / bsz;
		let off_in_blk = byte_off % bsz;
		let blk_data   = Self::read_block_bytes(dev, sb, bg.inode_table + block_idx as u32);
		if blk_data.len() < off_in_blk + 128 { return None; }
		let r = &blk_data[off_in_blk..off_in_blk + 128];

		let mode        = u16::from_le_bytes([r[0],  r[1]]);
		let links_count = u16::from_le_bytes([r[26], r[27]]);
		let size        = u32::from_le_bytes([r[4],  r[5],  r[6],  r[7]]);
		let blocks      = u32::from_le_bytes([r[28], r[29], r[30], r[31]]);
		let flags       = u32::from_le_bytes([r[32], r[33], r[34], r[35]]);
		let mut block   = [0u32; 15];
		for i in 0..15 {
			let base = 40 + i * 4;
			block[i] = u32::from_le_bytes([r[base], r[base+1], r[base+2], r[base+3]]);
		}
		Some(Inode { mode, links_count, size, blocks, flags, block, ino })
	}

	pub fn write(&self, dev: &dyn BlockDev, sb: &Superblock, bgdt: &BgDescTable) {
		let group      = sb.inode_block_group(self.ino) as usize;
		let idx        = sb.inode_local_index(self.ino) as usize;
		let isz        = sb.inode_size as usize;
		let bsz        = sb.block_size();
		let bg         = bgdt.get(group);
		let byte_off   = idx * isz;
		let block_idx  = byte_off / bsz;
		let off_in_blk = byte_off % bsz;
		let mut blk_data = Self::read_block_bytes(dev, sb, bg.inode_table + block_idx as u32);
		let dst = &mut blk_data[off_in_blk..off_in_blk + 128];
		dst[0..2].copy_from_slice(&self.mode.to_le_bytes());
		dst[26..28].copy_from_slice(&self.links_count.to_le_bytes());
		dst[4..8].copy_from_slice(&self.size.to_le_bytes());
		dst[28..32].copy_from_slice(&self.blocks.to_le_bytes());
		dst[32..36].copy_from_slice(&self.flags.to_le_bytes());
		for i in 0..15 {
			let base = 40 + i * 4;
			dst[base..base+4].copy_from_slice(&self.block[i].to_le_bytes());
		}
		Self::write_block_bytes(dev, sb, bg.inode_table + block_idx as u32, &blk_data);
	}
}
