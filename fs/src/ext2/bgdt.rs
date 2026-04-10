/*
 * ext2/bgdt.rs - Block Group Descriptor Table
 *
 * Each 32-byte entry describes one block group: where its bitmaps and
 * inode table live on disk and how many free blocks/inodes remain.
 */

extern crate alloc;
use alloc::vec::Vec;
use crate::BlockDev;
use super::superblock::Superblock;

/* ------------------------------------------------------------------ */
/*  On-disk descriptor (32 bytes)                                      */
/* ------------------------------------------------------------------ */

#[derive(Clone)]
pub struct BgDesc {
	pub block_bitmap: u32,   /* block number of block-usage bitmap */
	pub inode_bitmap: u32,   /* block number of inode-usage bitmap */
	pub inode_table:  u32,   /* block number of inode table start */
	pub free_blocks:  u16,
	pub free_inodes:  u16,
	pub used_dirs:    u16,
}

/* ------------------------------------------------------------------ */
/*  In-memory table                                                    */
/* ------------------------------------------------------------------ */

pub struct BgDescTable {
	pub entries: Vec<BgDesc>,
}

impl BgDescTable {
	/*
	 * read - Load all block group descriptors from disk.
	 */
	pub fn read(dev: &dyn BlockDev, sb: &Superblock) -> Self {
		let n       = sb.num_block_groups();
		let bsz     = sb.block_size();
		let spb     = sb.sectors_per_block();

		/*
		 * BGDT starts at the block immediately after the superblock block.
		 * Each descriptor is 32 bytes; they fill as many blocks as needed.
		 */
		let bgdt_start_sector = sb.block_to_sector(sb.bgdt_block());
		let total_bytes       = n * 32;
		let buf_blocks        = (total_bytes + bsz - 1) / bsz;

		let mut raw: Vec<u8> = Vec::with_capacity(buf_blocks * bsz);

		for blk in 0..buf_blocks as u64 {
			let sector_base = bgdt_start_sector + blk * spb;
			for s in 0..spb {
				let mut sec = [0u8; 512];
				dev.read_block(sector_base + s, &mut sec);
				raw.extend_from_slice(&sec);
			}
		}

		let mut entries = Vec::with_capacity(n);
		for i in 0..n {
			let off = i * 32;
			let e   = &raw[off..off + 32];
			entries.push(BgDesc {
				block_bitmap: u32::from_le_bytes([e[0], e[1], e[2], e[3]]),
				inode_bitmap: u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
				inode_table:  u32::from_le_bytes([e[8], e[9], e[10], e[11]]),
				free_blocks:  u16::from_le_bytes([e[12], e[13]]),
				free_inodes:  u16::from_le_bytes([e[14], e[15]]),
				used_dirs:    u16::from_le_bytes([e[16], e[17]]),
			});
		}

		BgDescTable { entries }
	}

	pub fn get(&self, group: usize) -> &BgDesc {
		&self.entries[group]
	}

	pub fn get_mut(&mut self, group: usize) -> &mut BgDesc {
		&mut self.entries[group]
	}

	/*
	 * write_entry - Flush one descriptor back to disk.
	 */
	pub fn write_entry(&self, dev: &dyn BlockDev, sb: &Superblock, group: usize) {
		let bgdt_start_sector = sb.block_to_sector(sb.bgdt_block());
		let spb               = sb.sectors_per_block();
		let bsz               = sb.block_size();

		/* Byte offset of this entry within the BGDT */
		let byte_off   = group * 32;
		let block_idx  = byte_off / bsz;
		let off_in_blk = byte_off % bsz;

		/* Read the entire block that contains this descriptor */
		let sector_base = bgdt_start_sector + block_idx as u64 * spb;
		let mut blk_buf: Vec<u8> = Vec::with_capacity(bsz);
		for s in 0..spb {
			let mut sec = [0u8; 512];
			dev.read_block(sector_base + s, &mut sec);
			blk_buf.extend_from_slice(&sec);
		}

		/* Patch the 32-byte entry in-place */
		let e   = &self.entries[group];
		let dst = &mut blk_buf[off_in_blk..off_in_blk + 32];
		dst[0..4].copy_from_slice(&e.block_bitmap.to_le_bytes());
		dst[4..8].copy_from_slice(&e.inode_bitmap.to_le_bytes());
		dst[8..12].copy_from_slice(&e.inode_table.to_le_bytes());
		dst[12..14].copy_from_slice(&e.free_blocks.to_le_bytes());
		dst[14..16].copy_from_slice(&e.free_inodes.to_le_bytes());
		dst[16..18].copy_from_slice(&e.used_dirs.to_le_bytes());
		/* bytes 18-31: padding, leave unchanged */

		/* Write back sector by sector */
		for s in 0..spb as usize {
			let mut sec = [0u8; 512];
			sec.copy_from_slice(&blk_buf[s * 512..(s + 1) * 512]);
			dev.write_block(sector_base + s as u64, &sec);
		}
	}
}
