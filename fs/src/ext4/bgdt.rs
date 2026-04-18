/*
 * ext4/bgdt.rs - Block Group Descriptor Table
 *
 * ext4 BGDTs can be 32 or 64 bytes per entry (desc_size field in superblock).
 * We read desc_size bytes but only use the lower 32 bytes (same layout as ext2).
 */

extern crate alloc;
use alloc::vec::Vec;
use crate::BlockDev;
use super::superblock::Superblock;

#[derive(Clone)]
pub struct BgDesc {
	pub block_bitmap:	u32,
	pub inode_bitmap:	u32,
	pub inode_table:	u32,
	pub free_blocks:	u16,
	pub free_inodes:	u16,
	pub used_dirs:		u16,
}

pub struct BgDescTable { pub entries: Vec<BgDesc> }

impl BgDescTable {
	pub fn read(dev: &dyn BlockDev, sb: &Superblock) -> Self {
		let n   = sb.num_block_groups();
		let bsz = sb.block_size();
		let spb = sb.sectors_per_block();
		let dsz = sb.desc_size as usize;          /* 32 or 64 bytes per entry */
		let bgdt_sector = sb.block_to_sector(sb.bgdt_block());
		let total_bytes = n * dsz;
		let buf_blocks  = (total_bytes + bsz - 1) / bsz;

		let mut raw: Vec<u8> = Vec::with_capacity(buf_blocks * bsz);
		for blk in 0..buf_blocks as u64 {
			let base = bgdt_sector + blk * spb;
			for s in 0..spb {
				let mut sec = [0u8; 512];
				dev.read_block(base + s, &mut sec);
				raw.extend_from_slice(&sec);
			}
		}

		let mut entries = Vec::with_capacity(n);
		for i in 0..n {
			let off = i * dsz;
			let e = &raw[off..off + 32];          /* always parse first 32 bytes */
			entries.push(BgDesc {
				block_bitmap: u32::from_le_bytes([e[0],  e[1],  e[2],  e[3]]),
				inode_bitmap: u32::from_le_bytes([e[4],  e[5],  e[6],  e[7]]),
				inode_table:  u32::from_le_bytes([e[8],  e[9],  e[10], e[11]]),
				free_blocks:  u16::from_le_bytes([e[12], e[13]]),
				free_inodes:  u16::from_le_bytes([e[14], e[15]]),
				used_dirs:    u16::from_le_bytes([e[16], e[17]]),
			});
		}
		BgDescTable { entries }
	}

	pub fn get(&self, g: usize) -> &BgDesc           { &self.entries[g] }
	pub fn get_mut(&mut self, g: usize) -> &mut BgDesc { &mut self.entries[g] }

	pub fn write_entry(&self, dev: &dyn BlockDev, sb: &Superblock, g: usize) {
		let dsz          = sb.desc_size as usize;
		let bsz          = sb.block_size();
		let spb          = sb.sectors_per_block();
		let bgdt_sector  = sb.block_to_sector(sb.bgdt_block());
		let byte_off     = g * dsz;
		let block_idx    = byte_off / bsz;
		let off_in_blk   = byte_off % bsz;
		let sector_base  = bgdt_sector + block_idx as u64 * spb;

		let mut buf: Vec<u8> = Vec::with_capacity(bsz);
		for s in 0..spb {
			let mut sec = [0u8; 512];
			dev.read_block(sector_base + s, &mut sec);
			buf.extend_from_slice(&sec);
		}
		let e   = &self.entries[g];
		let dst = &mut buf[off_in_blk..off_in_blk + 32];
		dst[0..4].copy_from_slice(&e.block_bitmap.to_le_bytes());
		dst[4..8].copy_from_slice(&e.inode_bitmap.to_le_bytes());
		dst[8..12].copy_from_slice(&e.inode_table.to_le_bytes());
		dst[12..14].copy_from_slice(&e.free_blocks.to_le_bytes());
		dst[14..16].copy_from_slice(&e.free_inodes.to_le_bytes());
		dst[16..18].copy_from_slice(&e.used_dirs.to_le_bytes());
		for s in 0..spb as usize {
			let mut sec = [0u8; 512];
			sec.copy_from_slice(&buf[s * 512..(s + 1) * 512]);
			dev.write_block(sector_base + s as u64, &sec);
		}
	}
}
