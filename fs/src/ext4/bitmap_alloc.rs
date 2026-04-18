/*
 * ext4/bitmap_alloc.rs - Block and Inode Bitmap Allocator
 *
 * Scans block/inode bitmaps to find and allocate free resources, and
 * clears bits to free them. After each operation the in-memory BGDT
 * counters and the superblock free counts are updated and written back.
 */

extern crate alloc;
use crate::BlockDev;
use super::superblock::Superblock;
use super::bgdt::BgDescTable;

/* ------------------------------------------------------------------ */
/*  Bitmap helpers                                                     */
/* ------------------------------------------------------------------ */

/* find_free_bit - Return index of first 0 bit in a byte slice, or None */
fn find_free_bit(bitmap: &[u8]) -> Option<usize> {
	for (byte_idx, &byte) in bitmap.iter().enumerate() {
		if byte != 0xFF {
			for bit in 0..8 {
				if byte & (1 << bit) == 0 {
					return Some(byte_idx * 8 + bit);
				}
			}
		}
	}
	None
}

fn set_bit(bitmap: &mut [u8], idx: usize) {
	bitmap[idx / 8] |= 1 << (idx % 8);
}

fn clear_bit(bitmap: &mut [u8], idx: usize) {
	bitmap[idx / 8] &= !(1 << (idx % 8));
}

/* ------------------------------------------------------------------ */
/*  Read/write one full block as a Vec<u8>                            */
/* ------------------------------------------------------------------ */

fn read_block(dev: &dyn BlockDev, sb: &Superblock, blk: u32) -> alloc::vec::Vec<u8> {
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(blk as u64);
	let bsz = sb.block_size();
	let mut out = alloc::vec::Vec::with_capacity(bsz);
	for s in 0..spb {
		let mut buf = [0u8; 512];
		dev.read_block(sec + s, &mut buf);
		out.extend_from_slice(&buf);
	}
	out
}

fn write_block(dev: &dyn BlockDev, sb: &Superblock, blk: u32, data: &[u8]) {
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(blk as u64);
	for s in 0..spb as usize {
		let mut buf = [0u8; 512];
		buf.copy_from_slice(&data[s * 512..(s + 1) * 512]);
		dev.write_block(sec + s as u64, &buf);
	}
}

/* ------------------------------------------------------------------ */
/*  Block alloc / free                                                 */
/* ------------------------------------------------------------------ */

/*
 * alloc_block - Find and mark a free block.
 *
 * Scans groups sequentially; returns the physical block number or None
 * if the filesystem is full.
 */
pub fn alloc_block(dev: &dyn BlockDev, sb: &mut Superblock, bgdt: &mut BgDescTable) -> Option<u32> {
	for g in 0..sb.num_block_groups() {
		let bg = bgdt.get(g);
		if bg.free_blocks == 0 { continue; }
		let bitmap_blk = bg.block_bitmap;

		let mut bitmap = read_block(dev, sb, bitmap_blk);
		if let Some(bit) = find_free_bit(&bitmap) {
			set_bit(&mut bitmap, bit);
			write_block(dev, sb, bitmap_blk, &bitmap);

			bgdt.get_mut(g).free_blocks -= 1;
			bgdt.write_entry(dev, sb, g);

			sb.free_blocks_lo = sb.free_blocks_lo.saturating_sub(1);
			sb.write_free_counts(dev);

			let phys = g as u32 * sb.blocks_per_group + bit as u32;
			return Some(phys);
		}
	}
	None
}

/*
 * free_block - Clear a block's bit in the bitmap.
 */
pub fn free_block(dev: &dyn BlockDev, sb: &mut Superblock, bgdt: &mut BgDescTable, block: u32) {
	let g       = (block / sb.blocks_per_group) as usize;
	let bit_idx = (block % sb.blocks_per_group) as usize;

	let bitmap_blk = bgdt.get(g).block_bitmap;
	let mut bitmap = read_block(dev, sb, bitmap_blk);
	clear_bit(&mut bitmap, bit_idx);
	write_block(dev, sb, bitmap_blk, &bitmap);

	bgdt.get_mut(g).free_blocks += 1;
	bgdt.write_entry(dev, sb, g);

	sb.free_blocks_lo += 1;
	sb.write_free_counts(dev);
}

/* ------------------------------------------------------------------ */
/*  Inode alloc / free                                                 */
/* ------------------------------------------------------------------ */

/*
 * alloc_inode - Find and mark a free inode.
 *
 * Returns the 1-based inode number or None if no free inodes exist.
 */
pub fn alloc_inode(dev: &dyn BlockDev, sb: &mut Superblock, bgdt: &mut BgDescTable) -> Option<u32> {
	for g in 0..sb.num_block_groups() {
		let bg = bgdt.get(g);
		if bg.free_inodes == 0 { continue; }
		let bitmap_blk = bg.inode_bitmap;

		let mut bitmap = read_block(dev, sb, bitmap_blk);
		if let Some(bit) = find_free_bit(&bitmap) {
			set_bit(&mut bitmap, bit);
			write_block(dev, sb, bitmap_blk, &bitmap);

			bgdt.get_mut(g).free_inodes -= 1;
			bgdt.write_entry(dev, sb, g);

			sb.free_inodes = sb.free_inodes.saturating_sub(1);
			sb.write_free_counts(dev);

			/* inode numbers are 1-based */
			let ino = g as u32 * sb.inodes_per_group + bit as u32 + 1;
			return Some(ino);
		}
	}
	None
}

/*
 * free_inode - Clear an inode's bit in the bitmap.
 */
pub fn free_inode(dev: &dyn BlockDev, sb: &mut Superblock, bgdt: &mut BgDescTable, ino: u32) {
	let g       = sb.inode_block_group(ino) as usize;
	let bit_idx = sb.inode_local_index(ino) as usize;

	let bitmap_blk = bgdt.get(g).inode_bitmap;
	let mut bitmap = read_block(dev, sb, bitmap_blk);
	clear_bit(&mut bitmap, bit_idx);
	write_block(dev, sb, bitmap_blk, &bitmap);

	bgdt.get_mut(g).free_inodes += 1;
	bgdt.write_entry(dev, sb, g);

	sb.free_inodes += 1;
	sb.write_free_counts(dev);
}
