/*
 * ext4/htree.rs - HTree Directory Indexing
 *
 * Implements the ext4 hash tree for O(log n) directory lookups.
 * Supports dx_root (root directories) and dx_node (indexed subdirectories).
 * Uses Jenkins one-at-a-time hash with ext4 mask format.
 *
 * On-disk structures:
 *   dx_root: inode(4) + reserved_zero(4) + hash_version(1) + info_len(1)
 *            + entries_count(2) + flevel(2) + dir_info(12)
 *   dx_node: reserved_zero(4) + hash_version(1) + info_len(1)
 *            + entries_count(2) + flevel(2) + dir_info(12)
 *   dx_entry: hash(4) + block(4)
 *   dx_dir_info: hash_version(1) + padding(1) + info_len(2) + flevel(2)
 *                + padding(6)
 */

extern crate alloc;
use super::bgdt::BgDescTable;
use super::extent;
use super::inode::Inode;
use super::superblock::Superblock;
use crate::BlockDev;

/* ------------------------------------------------------------------ */
/*  Constants                                                           */
/* ------------------------------------------------------------------ */

/* Hash format versions */
const HASH_VERSION_2: u8 = 2; /* 2-bit mask */
const HASH_VERSION_4: u8 = 4; /* 4-bit mask */
const HASH_VERSION_6: u8 = 6; /* 6-bit mask */
const HASH_VERSION_8: u8 = 8; /* 8-bit mask */

/* ------------------------------------------------------------------ */
/*  Hash function (Jenkins one-at-a-time, ext4 variant)                 */
/* ------------------------------------------------------------------ */

/*
 * hash_name - Compute ext4 hash for a directory entry name
 *
 * Uses Jenkins one-at-a-time hash with ext4-specific mask.
 * The mask size depends on the hash format version stored in the
 * directory block's dx_dir_info.
 */
pub fn hash_name(name: &[u8], hash_version: u8) -> u32 {
	let mut hash: u32 = 5381;
	for &b in name {
		hash = hash.wrapping_mul(33).wrapping_add(b as u32);
	}

	/* Apply mask based on hash format */
	let mask = match hash_version {
		HASH_VERSION_2 => 0x3,
		HASH_VERSION_4 => 0xF,
		HASH_VERSION_6 => 0x3F,
		HASH_VERSION_8 => 0xFF,
		_ => 0xFF, /* default to 8-bit mask */
	};

	hash & mask
}

/* ------------------------------------------------------------------ */
/*  Directory block parser                                              */
/* ------------------------------------------------------------------ */

/*
 * read_dir_block - Read a directory data block
 */
fn read_dir_block(dev: &dyn BlockDev, sb: &Superblock, blk: u32) -> alloc::vec::Vec<u8> {
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

/* ------------------------------------------------------------------ */
/*  dx_root / dx_node parser                                            */
/* ------------------------------------------------------------------ */

/*
 * parse_dx_root - Parse the dx_root header from a directory block
 *
 * Returns (hash_version, entries_count, leaf_block) or None if not indexed.
 *
 * The dx_root structure starts at offset 0 of the first directory block.
 * For dx_root (root directories):
 *   offset 0: parent inode (4 bytes) — we skip this
 *   offset 4: reserved_zero (4 bytes)
 *   offset 8: hash_version (1 byte)
 *   offset 9: info_len (1 byte)
 *   offset 10: entries_count (2 bytes)
 *   offset 12: flevel (2 bytes)
 *   offset 14: dir_info (12 bytes) — contains hash_version again
 *
 * After the dx_root header, dx_entry array follows.
 */
fn parse_dx_root(block: &[u8]) -> Option<(u8, u16, u32)> {
	/* Check if this is an indexed directory (hash_version != 0) */
	if block.len() < 26 {
		return None;
	}

	/* For dx_root: parent inode at 0, reserved at 4 */
	let hash_version = block[8];
	if hash_version == 0 {
		return None;
	}

	let info_len = u16::from_le_bytes([block[9], block[10]]);
	let entries_count = u16::from_le_bytes([block[10], block[11]]);

	/* dir_info starts at offset 14, contains hash_version at offset 14 */
	let dir_hash_version = block[14];
	if dir_hash_version == 0 {
		return None;
	}

	/* The first entry after dx_root header points to the leaf block */
	/* dx_root header size = 12 (header) + info_len */
	let header_size = 12 + info_len as usize;
	if block.len() < header_size + 8 {
		return None;
	}

	let leaf_block = u32::from_le_bytes([
		block[header_size],
		block[header_size + 1],
		block[header_size + 2],
		block[header_size + 3],
	]);

	Some((dir_hash_version, entries_count, leaf_block))
}

/*
 * parse_dx_node - Parse a dx_node index block
 *
 * Returns (hash_version, entries_count, leaf_block) or None.
 */
fn parse_dx_node(block: &[u8]) -> Option<(u8, u16, u32)> {
	if block.len() < 26 {
		return None;
	}

	/* dx_node: reserved_zero at 0 */
	let hash_version = block[4];
	if hash_version == 0 {
		return None;
	}

	let info_len = u16::from_le_bytes([block[5], block[6]]);
	let entries_count = u16::from_le_bytes([block[6], block[7]]);

	/* dir_info at offset 8 */
	let dir_hash_version = block[8];
	if dir_hash_version == 0 {
		return None;
	}

	let header_size = 12 + info_len as usize;
	if block.len() < header_size + 8 {
		return None;
	}

	let leaf_block = u32::from_le_bytes([
		block[header_size],
		block[header_size + 1],
		block[header_size + 2],
		block[header_size + 3],
	]);

	Some((dir_hash_version, entries_count, leaf_block))
}

/* ------------------------------------------------------------------ */
/*  Tree traversal                                                      */
/* ------------------------------------------------------------------ */

/*
 * traverse_tree - Walk the HTree to find the leaf block containing a hash
 *
 * @dev: Block device
 * @sb: Superblock
 * @dir_ino: Directory inode (for extent resolution)
 * @target_hash: Hash to search for
 * @first_data_blk: First data block of the directory
 *
 * Returns the physical block number of the leaf block, or None.
 */
pub fn traverse_tree(
	dev: &dyn BlockDev,
	sb: &Superblock,
	dir_ino: &Inode,
	target_hash: u32,
	first_data_blk: u32,
) -> Option<u32> {
	let bsz = sb.block_size();

	/* Read the first data block (may be dx_root) */
	let first_block = read_dir_block(dev, sb, first_data_blk);

	/* Try to parse as dx_root */
	if let Some((hash_version, _entries_count, leaf_block)) = parse_dx_root(&first_block) {
		return traverse_index_block(dev, sb, dir_ino, target_hash, leaf_block, hash_version, bsz);
	}

	/* Not indexed */
	None
}

/*
 * traverse_index_block - Recursively walk index blocks to find leaf
 */
fn traverse_index_block(
	dev: &dyn BlockDev,
	sb: &Superblock,
	dir_ino: &Inode,
	target_hash: u32,
	current_blk: u32,
	hash_version: u8,
	bsz: usize,
) -> Option<u32> {
	let block = read_dir_block(dev, sb, current_blk);

	/* Try to parse as dx_node */
	if let Some((_hash_version, _entries_count, child_block)) = parse_dx_node(&block) {
		/* Search entries for the right child */
		let info_len = u16::from_le_bytes([block[5], block[6]]);
		let entries_count = u16::from_le_bytes([block[6], block[7]]);
		let header_size = 12 + info_len as usize;

		let mut best_block: Option<u32> = None;
		let mut best_hash: u32 = u32::MAX;

		for i in 0..entries_count {
			let entry_off = header_size + (i as usize) * 8;
			if entry_off + 8 > block.len() {
				break;
			}
			let entry_hash = u32::from_le_bytes([
				block[entry_off],
				block[entry_off + 1],
				block[entry_off + 2],
				block[entry_off + 3],
			]);
			let entry_block = u32::from_le_bytes([
				block[entry_off + 4],
				block[entry_off + 5],
				block[entry_off + 6],
				block[entry_off + 7],
			]);

			if entry_hash <= target_hash && entry_hash < best_hash {
				best_hash = entry_hash;
				best_block = Some(entry_block);
			}
		}

		let next_blk = best_block?;

		/* Check if this is a leaf (no further index) */
		let child_block_data = read_dir_block(dev, sb, next_blk);
		if let Some((hv, _, _)) = parse_dx_node(&child_block_data) {
			if hv != 0 {
				/* Still an index node, recurse */
				return traverse_index_block(dev, sb, dir_ino, target_hash, next_blk, hv, bsz);
			}
		}

		Some(next_blk)
	} else {
		/* Not a valid index block, treat as leaf */
		Some(current_blk)
	}
}

/* ------------------------------------------------------------------ */
/*  Leaf lookup                                                         */
/* ------------------------------------------------------------------ */

/*
 * lookup_in_leaf - Linear scan of a leaf block for a name
 *
 * Reuses the existing for_each_entry logic from dir.rs.
 */
fn lookup_in_leaf(block: &[u8], name: &[u8]) -> Option<u32> {
	let mut off = 0usize;
	while off + 8 <= block.len() {
		let ino = u32::from_le_bytes([block[off], block[off + 1], block[off + 2], block[off + 3]]);
		let rec_len = u16::from_le_bytes([block[off + 4], block[off + 5]]) as usize;
		let nlen = block[off + 6] as usize;
		if rec_len < 8 || off + rec_len > block.len() {
			break;
		}
		let entry_name = &block[off + 8..off + 8 + nlen.min(block.len() - off - 8)];
		if ino != 0 && entry_name == name {
			return Some(ino);
		}
		off += rec_len;
	}
	None
}

/* ------------------------------------------------------------------ */
/*  Public API                                                          */
/* ------------------------------------------------------------------ */

/*
 * lookup_htree - HTree-based directory lookup
 *
 * 1. Read hash_version from dx_root header
 * 2. Compute hash of the name using hash_name()
 * 3. Traverse the hash tree to find the leaf block
 * 4. Linear scan the leaf for the matching name
 *
 * Returns the inode number of the entry, or None if not found.
 */
pub fn lookup_htree(
	dev: &dyn BlockDev,
	sb: &Superblock,
	dir_ino: &Inode,
	name: &str,
) -> Option<u32> {
	let name_bytes = name.as_bytes();

	/* Get the first data block of the directory */
	let bsz = sb.block_size();
	let n_blks = (dir_ino.size() + bsz - 1) / bsz;
	if n_blks == 0 {
		return None;
	}

	let first_data_blk = extent::get_block(dev, sb, &dir_ino.block, 0)?;

	/* Read the first block to extract hash_version from dx_root */
	let first_block = read_dir_block(dev, sb, first_data_blk);
	let (hash_version, _indexed) = match parse_dx_root(&first_block) {
		Some((hv, _, _)) => (hv, true),
		None => return None, /* Not an indexed directory */
	};
	let _ = _indexed; /* Used implicitly: if we got here, directory is indexed */

	/* Compute the hash of the name using the directory's hash version */
	let target_hash = hash_name(name_bytes, hash_version);

	/* Traverse the HTree with the computed hash */
	let leaf_blk = traverse_tree(dev, sb, dir_ino, target_hash, first_data_blk)?;

	/* Read the leaf block and scan for the name */
	let leaf = read_dir_block(dev, sb, leaf_blk);
	lookup_in_leaf(&leaf, name_bytes)
}

/*
 * is_htree_indexed - Check if a directory uses HTree indexing
 *
 * Reads the first data block and checks for a valid dx_root header.
 */
pub fn is_htree_indexed(dev: &dyn BlockDev, sb: &Superblock, dir_ino: &Inode) -> bool {
	let bsz = sb.block_size();
	let n_blks = (dir_ino.size() + bsz - 1) / bsz;
	if n_blks == 0 {
		return false;
	}

	let first_data_blk = match extent::get_block(dev, sb, &dir_ino.block, 0) {
		Some(b) => b,
		None => return false,
	};

	let block = read_dir_block(dev, sb, first_data_blk);
	parse_dx_root(&block).is_some()
}
