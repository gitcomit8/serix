/*
 * ext2/dir.rs - Directory Entry Parser
 *
 * Walks linear ext2 directory blocks to look up, list, add, and remove
 * directory entries. All entries are variable-length; rec_len is used to
 * advance through a block.
 */

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::BlockDev;
use super::superblock::Superblock;
use super::bgdt::BgDescTable;
use super::inode::Inode;

/* ------------------------------------------------------------------ */
/*  Entry file_type constants                                          */
/* ------------------------------------------------------------------ */

pub const EXT2_FT_REG_FILE: u8 = 1;
pub const EXT2_FT_DIR:      u8 = 2;

/* ------------------------------------------------------------------ */
/*  Block I/O helpers (duplicated locally to avoid cross-module dep)  */
/* ------------------------------------------------------------------ */

fn read_block(dev: &dyn BlockDev, sb: &Superblock, blk: u32) -> Vec<u8> {
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
/*  Entry iteration                                                    */
/* ------------------------------------------------------------------ */

/*
 * for_each_entry - Walk all entries in a directory block.
 *
 * `f` receives (offset_in_block, inode_num, rec_len, name_bytes).
 * Returns early if `f` returns false.
 */
fn for_each_entry<F>(block: &[u8], mut f: F)
where
	F: FnMut(usize, u32, usize, &[u8]) -> bool,
{
	let mut off = 0usize;
	while off + 8 <= block.len() {
		let ino      = u32::from_le_bytes([block[off], block[off+1], block[off+2], block[off+3]]);
		let rec_len  = u16::from_le_bytes([block[off+4], block[off+5]]) as usize;
		let name_len = block[off+6] as usize;

		if rec_len < 8 || off + rec_len > block.len() { break; }

		let name_bytes = &block[off + 8..off + 8 + name_len.min(block.len() - off - 8)];

		if !f(off, ino, rec_len, name_bytes) { return; }

		off += rec_len;
	}
}

/* ------------------------------------------------------------------ */
/*  Public API                                                         */
/* ------------------------------------------------------------------ */

/*
 * lookup_in_dir - Find entry by name; return child inode number.
 */
pub fn lookup_in_dir(
	dev: &dyn BlockDev,
	sb: &Superblock,
	bgdt: &BgDescTable,
	dir_ino: &Inode,
	name: &str,
) -> Option<u32> {
	let bsz = sb.block_size();
	let n_blocks = (dir_ino.size() + bsz - 1) / bsz;

	for blk_idx in 0..n_blocks as u32 {
		let phys = dir_ino.get_block(dev, sb, blk_idx)?;
		let block = read_block(dev, sb, phys);
		let mut found = None;

		for_each_entry(&block, |_off, ino, _rec, nbs| {
			if ino != 0 && nbs == name.as_bytes() {
				found = Some(ino);
				false
			} else {
				true
			}
		});

		if found.is_some() { return found; }
	}
	None
}

/*
 * readdir - List all entries in a directory.
 */
pub fn readdir(
	dev: &dyn BlockDev,
	sb: &Superblock,
	bgdt: &BgDescTable,
	dir_ino: &Inode,
) -> Vec<(String, vfs::FileType)> {
	let bsz = sb.block_size();
	let n_blocks = (dir_ino.size() + bsz - 1) / bsz;
	let mut entries = Vec::new();

	for blk_idx in 0..n_blocks as u32 {
		let phys = match dir_ino.get_block(dev, sb, blk_idx) { Some(p) => p, None => continue };
		let block = read_block(dev, sb, phys);

		for_each_entry(&block, |_off, ino, _rec, nbs| {
			if ino != 0 {
				if let Ok(s) = core::str::from_utf8(nbs) {
					let name = String::from(s);
					if name != "." && name != ".." {
						let ftype = if let Some(ch_ino) = super::inode::Inode::read(dev, sb, bgdt, ino) {
							if ch_ino.is_dir() { vfs::FileType::Directory } else { vfs::FileType::File }
						} else {
							vfs::FileType::File
						};
						entries.push((name, ftype));
					}
				}
			}
			true
		});
	}
	entries
}

/*
 * add_entry - Append a new directory entry (name → inode) to a directory.
 *
 * Tries to fit in the last block's free tail first; allocates a new block
 * if necessary.
 */
pub fn add_entry(
	dev: &dyn BlockDev,
	sb: &mut Superblock,
	bgdt: &mut BgDescTable,
	dir_ino: &mut Inode,
	name: &str,
	child_ino: u32,
	file_type: u8,
) {
	let bsz     = sb.block_size();
	let name_b  = name.as_bytes();
	let nlen    = name_b.len();
	/* Entry size: header(8) + name, rounded to 4 bytes */
	let needed  = ((8 + nlen + 3) & !3) as u16;

	let n_blocks = (dir_ino.size() + bsz - 1) / bsz;

	/* Scan existing blocks for slack space in the last real entry */
	for blk_idx in 0..n_blocks as u32 {
		let phys = match dir_ino.get_block(dev, sb, blk_idx) { Some(p) => p, None => continue };
		let mut block = read_block(dev, sb, phys);
		let mut inserted = false;

		/* Walk entries to find the last one with surplus rec_len */
		let mut off = 0usize;
		loop {
			if off + 8 > block.len() { break; }
			let rec_len  = u16::from_le_bytes([block[off+4], block[off+5]]) as usize;
			let name_len = block[off+6] as usize;
			if rec_len < 8 { break; }

			let actual = ((8 + name_len + 3) & !3) as usize;
			let next_off = off + rec_len;

			if next_off >= block.len() {
				/* This is the last entry in the block */
				let slack = rec_len - actual;
				if slack >= needed as usize {
					/* Shrink current entry to actual size, put new entry after */
					block[off+4..off+6].copy_from_slice(&(actual as u16).to_le_bytes());
					let new_off = off + actual;
					let new_rec = (rec_len - actual) as u16;
					write_dir_entry(&mut block, new_off, child_ino, new_rec, nlen as u8, file_type, name_b);
					write_block(dev, sb, phys, &block);
					inserted = true;
				}
				break;
			}
			off = next_off;
		}

		if inserted { return; }
	}

	/* No space found — allocate a new block */
	let phys = match super::bitmap_alloc::alloc_block(dev, sb, bgdt) { Some(p) => p, None => return };
	let blk_idx = n_blocks as u32;
	let mut block = alloc::vec![0u8; bsz];
	let rec_len = bsz as u16;
	write_dir_entry(&mut block, 0, child_ino, rec_len, nlen as u8, file_type, name_b);
	write_block(dev, sb, phys, &block);

	dir_ino.set_block(dev, sb, bgdt, blk_idx, phys);
	dir_ino.size += bsz as u32;
	dir_ino.blocks += sb.sectors_per_block() as u32;
	dir_ino.write(dev, sb, bgdt);
}

/*
 * remove_entry - Delete entry `name` from a directory.
 *
 * Sets the entry's inode to 0 and merges its rec_len into the previous
 * entry (or marks it as a deleted stub if it's the first in the block).
 */
pub fn remove_entry(
	dev: &dyn BlockDev,
	sb: &Superblock,
	bgdt: &BgDescTable,
	dir_ino: &Inode,
	name: &str,
) -> bool {
	let bsz = sb.block_size();
	let n_blocks = (dir_ino.size() + bsz - 1) / bsz;

	for blk_idx in 0..n_blocks as u32 {
		let phys = match dir_ino.get_block(dev, sb, blk_idx) { Some(p) => p, None => continue };
		let mut block = read_block(dev, sb, phys);
		let mut prev_off: Option<usize> = None;
		let mut off = 0usize;
		let mut found_off = None;

		for_each_entry(&block, |cur_off, ino, rec, nbs| {
			if ino != 0 && nbs == name.as_bytes() {
				found_off = Some((cur_off, prev_off, rec));
				false
			} else {
				prev_off = Some(cur_off);
				true
			}
		});

		if let Some((cur_off, prev, rec)) = found_off {
			/* Zero the inode field — marks entry as free */
			block[cur_off..cur_off+4].copy_from_slice(&0u32.to_le_bytes());
			if let Some(poff) = prev {
				/* Expand previous entry's rec_len to absorb this one */
				let prev_rec = u16::from_le_bytes([block[poff+4], block[poff+5]]) as usize;
				let new_rec  = (prev_rec + rec) as u16;
				block[poff+4..poff+6].copy_from_slice(&new_rec.to_le_bytes());
			}
			write_block(dev, sb, phys, &block);
			return true;
		}
	}
	false
}

/* ------------------------------------------------------------------ */
/*  Internal write helper                                              */
/* ------------------------------------------------------------------ */

fn write_dir_entry(
	block: &mut [u8],
	off: usize,
	ino: u32,
	rec_len: u16,
	name_len: u8,
	file_type: u8,
	name: &[u8],
) {
	block[off..off+4].copy_from_slice(&ino.to_le_bytes());
	block[off+4..off+6].copy_from_slice(&rec_len.to_le_bytes());
	block[off+6] = name_len;
	block[off+7] = file_type;
	let nlen = name_len as usize;
	block[off+8..off+8+nlen].copy_from_slice(&name[..nlen]);
}
