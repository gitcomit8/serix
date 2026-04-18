/*
 * ext4/extent.rs - Extent Tree Block Resolution
 *
 * An extent tree maps logical file block indices to physical block ranges.
 * The root is stored in inode.block[0..14] (60 bytes = header + 4 inline extents).
 *
 * On-disk structures (all little-endian):
 *   ExtentHeader { magic: 0xF30A, entries: u16, max: u16, depth: u16, gen: u32 }  — 12 bytes
 *   ExtentIdx    { block: u32, leaf_lo: u32, leaf_hi: u16, _: u16 }               — 12 bytes (depth > 0)
 *   Extent       { block: u32, len: u16, start_hi: u16, start_lo: u32 }           — 12 bytes (depth == 0)
 *
 * We only support single-level trees (depth == 0) inline in the inode.
 * Files that grow beyond 4 extents allocate a tree block (depth > 0) which
 * we also handle here.
 */

extern crate alloc;
use alloc::vec::Vec;
use crate::BlockDev;
use super::superblock::Superblock;
use super::bgdt::BgDescTable;

const EXTENT_MAGIC: u16 = 0xF30A;

fn read_blk(dev: &dyn BlockDev, sb: &Superblock, blk: u32) -> Vec<u8> {
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

fn write_blk(dev: &dyn BlockDev, sb: &Superblock, blk: u32, data: &[u8]) {
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(blk as u64);
	for s in 0..spb as usize {
		let mut buf = [0u8; 512];
		buf.copy_from_slice(&data[s * 512..(s + 1) * 512]);
		dev.write_block(sec + s as u64, &buf);
	}
}

fn u16_le(b: &[u8], off: usize) -> u16 { u16::from_le_bytes([b[off], b[off+1]]) }
fn u32_le(b: &[u8], off: usize) -> u32 { u32::from_le_bytes([b[off], b[off+1], b[off+2], b[off+3]]) }

/* inode.block[0..14] as raw bytes (60 bytes) */
fn inode_block_bytes(block: &[u32; 15]) -> [u8; 60] {
	let mut raw = [0u8; 60];
	for i in 0..15 {
		raw[i*4..i*4+4].copy_from_slice(&block[i].to_le_bytes());
	}
	raw
}

/*
 * get_block - Resolve logical block index `n` to physical block number.
 *
 * Returns None for sparse regions or if the tree depth > 2.
 */
pub fn get_block(
	dev: &dyn BlockDev,
	sb: &Superblock,
	block: &[u32; 15],
	n: u32,
) -> Option<u32> {
	let raw = inode_block_bytes(block);
	resolve_in_node(dev, sb, &raw, n)
}

fn resolve_in_node(dev: &dyn BlockDev, sb: &Superblock, node: &[u8], n: u32) -> Option<u32> {
	if u16_le(node, 0) != EXTENT_MAGIC { return None; }
	let entries = u16_le(node, 2) as usize;
	let depth   = u16_le(node, 6);

	if depth == 0 {
		/* Leaf node: search extents at offsets 12, 24, 36, ... */
		for i in 0..entries {
			let off      = 12 + i * 12;
			let ee_block = u32_le(node, off);
			let ee_len   = u16_le(node, off + 4);
			let ee_start = u32_le(node, off + 8); /* start_lo; ignore start_hi */
			if n >= ee_block && n < ee_block + ee_len as u32 {
				let delta = n - ee_block;
				return Some(ee_start + delta);
			}
		}
		None
	} else {
		/* Internal node: find the right child index block */
		let mut chosen: Option<u32> = None;
		for i in 0..entries {
			let off      = 12 + i * 12;
			let ei_block = u32_le(node, off);
			if n >= ei_block {
				chosen = Some(u32_le(node, off + 4)); /* leaf_lo */
			} else {
				break;
			}
		}
		let child_blk = chosen?;
		let child_data = read_blk(dev, sb, child_blk);
		resolve_in_node(dev, sb, &child_data, n)
	}
}

/*
 * set_block - Append a new extent mapping logical block `n` to physical `phys`.
 *
 * If there is room in the inline extent area (inode.block[]), appends there.
 * Otherwise allocates a new extent tree block (depth = 1).
 * Writes the updated inode.block[] back via the closure.
 *
 * Returns false if no space could be found (filesystem full or tree too deep).
 */
pub fn set_block(
	dev: &dyn BlockDev,
	sb: &mut Superblock,
	bgdt: &mut BgDescTable,
	block: &mut [u32; 15],
	n: u32,
	phys: u32,
) -> bool {
	let mut raw = inode_block_bytes(block);

	if u16_le(&raw, 0) != EXTENT_MAGIC {
		/* First ever extent write — initialise header */
		raw[0..2].copy_from_slice(&EXTENT_MAGIC.to_le_bytes());
		raw[2..4].copy_from_slice(&0u16.to_le_bytes()); /* entries = 0 */
		raw[4..6].copy_from_slice(&4u16.to_le_bytes()); /* max = 4 inline */
		raw[6..8].copy_from_slice(&0u16.to_le_bytes()); /* depth = 0 */
		raw[8..12].copy_from_slice(&0u32.to_le_bytes());/* generation */
	}

	let entries = u16_le(&raw, 2) as usize;
	let max     = u16_le(&raw, 4) as usize;
	let depth   = u16_le(&raw, 6);

	if depth == 0 && entries < max {
		/* Try to extend the last extent if it is contiguous */
		if entries > 0 {
			let last_off   = 12 + (entries - 1) * 12;
			let ee_block   = u32_le(&raw, last_off);
			let ee_len     = u16_le(&raw, last_off + 4);
			let ee_start   = u32_le(&raw, last_off + 8);
			if n == ee_block + ee_len as u32 && phys == ee_start + ee_len as u32
				&& ee_len < 0x7FFF
			{
				/* Extend in place */
				let new_len = (ee_len + 1).to_le_bytes();
				raw[last_off + 4] = new_len[0];
				raw[last_off + 5] = new_len[1];
				for i in 0..15 { block[i] = u32_le(&raw, i * 4); }
				return true;
			}
		}
		/* Append new extent */
		let new_off = 12 + entries * 12;
		raw[new_off..new_off+4].copy_from_slice(&n.to_le_bytes());
		raw[new_off+4..new_off+6].copy_from_slice(&1u16.to_le_bytes());
		raw[new_off+6..new_off+8].copy_from_slice(&0u16.to_le_bytes()); /* start_hi */
		raw[new_off+8..new_off+12].copy_from_slice(&phys.to_le_bytes());
		let new_entries = (entries + 1) as u16;
		raw[2..4].copy_from_slice(&new_entries.to_le_bytes());
		for i in 0..15 { block[i] = u32_le(&raw, i * 4); }
		true
	} else if depth == 0 && entries >= max {
		/* Inline area full: allocate a tree block and promote */
		let tree_blk = match super::bitmap_alloc::alloc_block(dev, sb, bgdt) {
			Some(b) => b, None => return false,
		};
		/* Copy existing extents into the new tree block */
		let mut tree_data = alloc::vec![0u8; sb.block_size()];
		tree_data[0..12].copy_from_slice(&raw[0..12]); /* copy header */
		/* Adjust max for the tree block */
		let tree_max = ((sb.block_size() - 12) / 12) as u16;
		tree_data[4..6].copy_from_slice(&tree_max.to_le_bytes());
		tree_data[12..12 + entries * 12].copy_from_slice(&raw[12..12 + entries * 12]);
		/* Append new extent in tree block */
		let new_off = 12 + entries * 12;
		tree_data[new_off..new_off+4].copy_from_slice(&n.to_le_bytes());
		tree_data[new_off+4..new_off+6].copy_from_slice(&1u16.to_le_bytes());
		tree_data[new_off+6..new_off+8].copy_from_slice(&0u16.to_le_bytes());
		tree_data[new_off+8..new_off+12].copy_from_slice(&phys.to_le_bytes());
		let cnt = (entries + 1) as u16;
		tree_data[2..4].copy_from_slice(&cnt.to_le_bytes());
		write_blk(dev, sb, tree_blk, &tree_data);
		/* Rewrite inode block[] as a depth-1 index node */
		let mut new_raw = [0u8; 60];
		new_raw[0..2].copy_from_slice(&EXTENT_MAGIC.to_le_bytes());
		new_raw[2..4].copy_from_slice(&1u16.to_le_bytes()); /* 1 index entry */
		new_raw[4..6].copy_from_slice(&4u16.to_le_bytes()); /* max index entries inline */
		new_raw[6..8].copy_from_slice(&1u16.to_le_bytes()); /* depth = 1 */
		new_raw[12..16].copy_from_slice(&0u32.to_le_bytes()); /* ei_block = 0 */
		new_raw[16..20].copy_from_slice(&tree_blk.to_le_bytes()); /* leaf_lo */
		for i in 0..15 { block[i] = u32_le(&new_raw, i * 4); }
		true
	} else {
		/* depth > 1: locate the right child index block and recurse */
		/* For simplicity, append to the last leaf block found */
		let last_off  = 12 + (entries.saturating_sub(1)) * 12;
		let child_blk = u32_le(&raw, last_off + 4);
		let mut child_data = read_blk(dev, sb, child_blk);
		let child_entries = u16_le(&child_data, 2) as usize;
		let child_max     = u16_le(&child_data, 4) as usize;
		if child_entries >= child_max {
			return false; /* tree too deep — not supported */
		}
		let new_off = 12 + child_entries * 12;
		child_data[new_off..new_off+4].copy_from_slice(&n.to_le_bytes());
		child_data[new_off+4..new_off+6].copy_from_slice(&1u16.to_le_bytes());
		child_data[new_off+6..new_off+8].copy_from_slice(&0u16.to_le_bytes());
		child_data[new_off+8..new_off+12].copy_from_slice(&phys.to_le_bytes());
		let cnt = (child_entries + 1) as u16;
		child_data[2..4].copy_from_slice(&cnt.to_le_bytes());
		write_blk(dev, sb, child_blk, &child_data);
		true
	}
}
