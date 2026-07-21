/*
 * ext4/format.rs - ext4 Filesystem Formatter (mkfs)
 *
 * Writes a minimal ext4 filesystem to a BlockDev:
 *   - Superblock (sectors 2-3)
 *   - Block Group Descriptor Table (one group)
 *   - Block bitmap (all blocks free except group metadata)
 *   - Inode bitmap (inodes 1-2 used: unallocated + root)
 *   - Inode table (inode 2 = root directory)
 *   - Root directory block (".", "..")
 *
 * Layout (single block group):
 *   Group 0:
 *     Block 0: reserved (superblock lives at offset 1024 = sectors 2-3)
 *     Block 1: reserved
 *     Block 2-3: superblock
 *     Block 4: BGDT
 *     Block 5: block bitmap
 *     Block 6: inode bitmap
 *     Block 7+: inode table
 *     Block (7 + inode_table_blocks): first data block
 */

extern crate alloc;
use super::bgdt::BgDesc;
use super::superblock::{Superblock, EXT4_MAGIC, INCOMPAT_EXTENTS};
use crate::BlockDev;
use alloc::vec::Vec;

/* ------------------------------------------------------------------ */
/*  Constants                                                           */
/* ------------------------------------------------------------------ */

const EXT4_S_IFDIR: u16 = 0x4000;
const EXT4_S_IFREG: u16 = 0x8000;
const EXT4_EXTENTS_FL: u32 = 0x0008_0000;
const EXT4_FT_DIR: u8 = 2;
const EXT4_FT_REG_FILE: u8 = 1;

/* ------------------------------------------------------------------ */
/*  Helpers                                                             */
/* ------------------------------------------------------------------ */

fn zero_block(dev: &dyn BlockDev, sb: &Superblock, blk: u32) {
	let zeros = [0u8; 512];
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(blk as u64);
	for s in 0..spb {
		dev.write_block(sec + s, &zeros);
	}
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
/*  Layout calculation                                                  */
/* ------------------------------------------------------------------ */

/*
 * LayoutResult - computed layout for format_device
 */
pub struct LayoutResult {
	pub block_size: usize,
	pub inode_count: u32,
	pub block_count: u32,
	pub blocks_per_group: u32,
	pub inodes_per_group: u32,
	pub num_block_groups: u32,
	pub bgdt_block: u32,
	pub block_bitmap_block: u32,
	pub inode_bitmap_block: u32,
	pub inode_table_block: u32,
	pub inode_table_blocks: u32,
	pub journal_block: u32,
	pub journal_blocks: u32,
	pub first_data_block: u32,
	pub first_ino: u32,
}

pub fn calculate_layout(
	block_size: usize,
	inode_count: u32,
	block_count: u32,
) -> LayoutResult {
	let blocks_per_group = block_count;
	let inodes_per_group = inode_count;
	let num_block_groups = 1u32;

	/* Superblock: sectors 2-3 (1024 bytes) */
	/* BGDT: one entry, fits in one block */
	let bgdt_block = 4;

	/* Block bitmap: one block */
	let block_bitmap_block = bgdt_block + 1;

	/* Inode bitmap: one block */
	let inode_bitmap_block = block_bitmap_block + 1;

	/* Inode table: inode_count * inode_size / block_size */
	let inode_size = 256u32;
	let inode_table_blocks = ((inode_count as u32) * inode_size / block_size as u32)
		.max(1);
	let inode_table_block = inode_bitmap_block + 1;

	/* Journal: fixed JOURNAL_BLOCKS after inode table */
	use super::journal::JOURNAL_BLOCKS;
	let journal_block = inode_table_block + inode_table_blocks;
	let journal_blocks = JOURNAL_BLOCKS;

	/* First data block: after journal */
	let first_data_block = journal_block + journal_blocks;

	LayoutResult {
		block_size,
		inode_count,
		block_count,
		blocks_per_group,
		inodes_per_group,
		num_block_groups,
		bgdt_block,
		block_bitmap_block,
		inode_bitmap_block,
		inode_table_block,
		inode_table_blocks,
		journal_block,
		journal_blocks,
		first_data_block,
		first_ino: 11, /* reserved inodes */
	}
}

/* ------------------------------------------------------------------ */
/*  Superblock writer                                                   */
/* ------------------------------------------------------------------ */

fn write_superblock(dev: &dyn BlockDev, layout: &LayoutResult) {
	let bsz = layout.block_size;
	let spb = (bsz / 512) as u64;

	/* Read sectors 2-3 into a 1024-byte buffer */
	let mut raw = alloc::vec![0u8; bsz];
	let s2 = alloc::vec![0u8; 512];
	let s3 = alloc::vec![0u8; 512];
	raw[..512].copy_from_slice(&s2);
	raw[512..].copy_from_slice(&s3);

	/* s_magic (offset 56) */
	raw[56..58].copy_from_slice(&EXT4_MAGIC.to_le_bytes());

	/* s_inodes_count (offset 0) */
	raw[0..4].copy_from_slice(&layout.inode_count.to_le_bytes());

	/* s_blocks_count_lo (offset 4) */
	raw[4..8].copy_from_slice(&layout.block_count.to_le_bytes());

	/* s_free_blocks_lo (offset 12) */
	let free_blocks = layout.block_count - layout.first_data_block;
	raw[12..16].copy_from_slice(&free_blocks.to_le_bytes());

	/* s_free_inodes (offset 16) */
	raw[16..20].copy_from_slice(
		&layout
			.inode_count
			.saturating_sub(2) /* inodes 1,2 used */
			.to_le_bytes(),
	);

	/* s_first_data_block (offset 20) */
	raw[20..24].copy_from_slice(&0u32.to_le_bytes()); /* block 0 is reserved */

	/* s_log_block_size (offset 24) */
	let log_block_size = bsz.trailing_zeros();
	raw[24..28].copy_from_slice(&log_block_size.to_le_bytes());

	/* s_blocks_per_group (offset 32) */
	raw[32..36].copy_from_slice(&layout.blocks_per_group.to_le_bytes());

	/* s_inodes_per_group (offset 40) */
	raw[40..44].copy_from_slice(&layout.inodes_per_group.to_le_bytes());

	/* s_inode_size (offset 88) */
	raw[88..90].copy_from_slice(&256u16.to_le_bytes());

	/* s_first_ino (offset 84) */
	raw[84..88].copy_from_slice(&layout.first_ino.to_le_bytes());

	/* s_feature_incompat (offset 96) */
	raw[96..100].copy_from_slice(&INCOMPAT_EXTENTS.to_le_bytes());

	/* s_feature_compat (offset 92) — has_journal */
	raw[92..96].copy_from_slice(&super::superblock::COMPAT_HAS_JOURNAL.to_le_bytes());

	/* s_desc_size (offset 254) */
	raw[254..256].copy_from_slice(&32u16.to_le_bytes());

	/* Write back to sectors 2-3 */
	let mut buf = alloc::vec![0u8; bsz];
	buf[..512].copy_from_slice(&raw[..512]);
	buf[512..].copy_from_slice(&raw[512..]);
	let sec = 2u64;
	for s in 0..spb {
		let mut sector_buf = [0u8; 512];
		let start = (s * 512u64) as usize;
		let end = ((s + 1) * 512u64) as usize;
		sector_buf.copy_from_slice(&buf[start..end]);
		dev.write_block(sec + s, &sector_buf);
	}
}

/* ------------------------------------------------------------------ */
/*  Block Group Descriptor writer                                       */
/* ------------------------------------------------------------------ */

fn write_bgdt(dev: &dyn BlockDev, layout: &LayoutResult) {
	let sb = Superblock::fake(layout);
	let bsz = layout.block_size;

	/* Single block group descriptor (32 bytes) */
	let mut entry = [0u8; 32];
	entry[0..4].copy_from_slice(&layout.block_bitmap_block.to_le_bytes());
	entry[4..8].copy_from_slice(&layout.inode_bitmap_block.to_le_bytes());
	entry[8..12].copy_from_slice(&layout.inode_table_block.to_le_bytes());

	let free_blocks = layout.block_count - layout.first_data_block;
	entry[12..14].copy_from_slice(&(free_blocks as u16).to_le_bytes());
	entry[14..16].copy_from_slice(
		&(layout.inode_count.saturating_sub(2) as u16).to_le_bytes(),
	);
	entry[16..18].copy_from_slice(&1u16.to_le_bytes()); /* used_dirs = 1 (root) */

	/* BGDT lives in one block */
	let mut bgdt_blk = alloc::vec![0u8; bsz];
	bgdt_blk[..32].copy_from_slice(&entry);

	write_block(dev, &sb, layout.bgdt_block, &bgdt_blk);
}

/* ------------------------------------------------------------------ */
/*  Bitmap initialisation                                               */
/* ------------------------------------------------------------------ */

fn init_block_bitmap(dev: &dyn BlockDev, layout: &LayoutResult) {
	let bsz = layout.block_size;
	let mut bitmap = alloc::vec![0u8; bsz];

	/* Reserve blocks 0-6 (superblock, BGDT, bitmaps, inode table) */
	for b in 0..layout.first_data_block {
		let byte = (b / 8) as usize;
		let bit = (b % 8) as usize;
		bitmap[byte] |= 1 << bit;
	}

	write_block(dev, &Superblock::fake(layout), layout.block_bitmap_block, &bitmap);
}

fn init_inode_bitmap(dev: &dyn BlockDev, layout: &LayoutResult) {
	let bsz = layout.block_size;
	let mut bitmap = alloc::vec![0u8; bsz];

	/* Reserve inodes 1 (unallocated) and 2 (root) */
	for ino in [1u32, 2] {
		let byte = (ino as usize / 8) % bsz;
		let bit = (ino as usize % 8) as usize;
		bitmap[byte] |= 1 << bit;
	}

	write_block(dev, &Superblock::fake(layout), layout.inode_bitmap_block, &bitmap);
}

/* ------------------------------------------------------------------ */
/*  Inode table initialisation                                          */
/* ------------------------------------------------------------------ */

fn init_inode_table(dev: &dyn BlockDev, layout: &LayoutResult) {
	let sb = Superblock::fake(layout);
	let bsz = layout.block_size;
	let inode_size = 256u32;
	let inodes_per_block = bsz / inode_size as usize;

	/* Zero all inode table blocks */
	for blk in 0..layout.inode_table_blocks {
		zero_block(dev, &sb, layout.inode_table_block + blk);
	}

	/* Write root inode (inode 2) */
	let root_ino: usize = 2;
	let root_blk = root_ino / inodes_per_block;
	let root_off = (root_ino % inodes_per_block) * inode_size as usize;

	let mut inode_blk = alloc::vec![0u8; bsz];
	/* mode */
	inode_blk[root_off..root_off + 2].copy_from_slice(&EXT4_S_IFDIR.to_le_bytes());
	/* links_count (offset 26) */
	inode_blk[root_off + 26..root_off + 28].copy_from_slice(&2u16.to_le_bytes());
	/* flags (offset 32) */
	inode_blk[root_off + 32..root_off + 36].copy_from_slice(&EXT4_EXTENTS_FL.to_le_bytes());

	/* Write the inode block */
	let mut data = alloc::vec![0u8; bsz];
	data[..bsz.min(512)].copy_from_slice(&inode_blk[..bsz.min(512)]);
	write_block(dev, &sb, layout.inode_table_block + root_blk as u32, &data);
}

/* ------------------------------------------------------------------ */
/*  Root directory creation                                             */
/* ------------------------------------------------------------------ */

fn create_root_dir(dev: &dyn BlockDev, layout: &LayoutResult) {
	let sb = Superblock::fake(layout);
	let bsz = layout.block_size;

	/* Allocate first data block as root directory */
	let root_blk = layout.first_data_block;
	let mut dir_blk = alloc::vec![0u8; bsz];

	/* Entry 1: "." -> inode 2 */
	dir_blk[0..4].copy_from_slice(&2u32.to_le_bytes());
	dir_blk[4..6].copy_from_slice(&12u16.to_le_bytes()); /* rec_len */
	dir_blk[6] = 1; /* name length */
	dir_blk[7] = EXT4_FT_DIR;
	dir_blk[8] = b'.';

	/* Entry 2: ".." -> inode 2 */
	let rem = bsz - 12;
	dir_blk[12..16].copy_from_slice(&2u32.to_le_bytes());
	dir_blk[16..18].copy_from_slice(&(rem as u16).to_le_bytes());
	dir_blk[18] = 2; /* name length */
	dir_blk[19] = EXT4_FT_DIR;
	dir_blk[20] = b'.';
	dir_blk[21] = b'.';

	write_block(dev, &sb, root_blk, &dir_blk);
}

/* ------------------------------------------------------------------ */
/*  Public API                                                          */
/* ------------------------------------------------------------------ */

/*
 * format_device - Write a minimal ext4 filesystem to a BlockDev
 *
 * @dev: Block device to format (must be large enough)
 * @block_size: Block size in bytes (must be power of 2, 1024-65536)
 * @inode_count: Total number of inodes (must be >= 512)
 * @block_count: Total number of data blocks (excludes superblock/BGDT)
 *
 * Returns Err if the device is too small or parameters are invalid.
 */
pub fn format_device(
	dev: &dyn BlockDev,
	block_size: usize,
	inode_count: u32,
	block_count: u32,
) -> Result<(), &'static str> {
	/* Validate parameters */
	if block_size < 1024 || block_size > 65536 {
		return Err("block_size must be 1024-65536");
	}
	if !block_size.is_power_of_two() {
		return Err("block_size must be power of 2");
	}
	if inode_count < 512 {
		return Err("inode_count must be >= 512");
	}
	if block_count < 1024 {
		return Err("block_count must be >= 1024");
	}

	let layout = calculate_layout(block_size, inode_count, block_count);

	/* Check device has enough sectors */
	let needed_sectors = (layout.first_data_block + 1) * (block_size / 512) as u32;
	if dev.sector_count() < needed_sectors as u64 {
		return Err("device too small for requested layout");
	}

	/* 1. Write superblock */
	write_superblock(dev, &layout);

	/* 2. Write BGDT */
	write_bgdt(dev, &layout);

	/* 3. Zero and init block bitmap */
	let sb = Superblock::fake(&layout);
	zero_block(dev, &sb, layout.block_bitmap_block);
	init_block_bitmap(dev, &layout);

	/* 4. Zero and init inode bitmap */
	zero_block(dev, &sb, layout.inode_bitmap_block);
	init_inode_bitmap(dev, &layout);

	/* 5. Zero and init inode table */
	for blk in 0..layout.inode_table_blocks {
		zero_block(dev, &sb, layout.inode_table_block + blk);
	}
	init_inode_table(dev, &layout);

	/* 6. Write journal superblock */
	let jsb = super::journal::JournalSuperblock::new(
		layout.block_size as u32,
		layout.journal_block,
		layout.journal_blocks,
	);
	let jsb_bytes = jsb.serialize();
	/* Write journal superblock to first journal block */
	let mut jsb_buf = alloc::vec![0u8; layout.block_size];
	jsb_buf[..jsb_bytes.len()].copy_from_slice(&jsb_bytes);
	write_block(dev, &sb, layout.journal_block, &jsb_buf);
	/* Zero remaining journal blocks */
	for blk in 1..layout.journal_blocks {
		zero_block(dev, &sb, layout.journal_block + blk);
	}

	/* 7. Create root directory */
	create_root_dir(dev, &layout);

	Ok(())
}

/* ------------------------------------------------------------------ */
/*  Superblock fake helper (format doesn't need a real superblock)      */
/* ------------------------------------------------------------------ */

/*
 * Superblock::fake - Create a temporary Superblock for format operations.
 *
 * format.rs writes the superblock; it doesn't need to read one first.
 * This provides the helper methods (block_size, block_to_sector, etc.)
 * without requiring a valid on-disk superblock.
 */
impl Superblock {
	pub fn fake(layout: &LayoutResult) -> Self {
		Superblock {
			inodes_count: layout.inode_count,
			blocks_count_lo: layout.block_count,
			free_blocks_lo: layout.block_count - layout.first_data_block,
			free_inodes: layout.inode_count.saturating_sub(2),
			first_data_block: 0,
			log_block_size: layout.block_size.trailing_zeros(),
			blocks_per_group: layout.blocks_per_group,
			inodes_per_group: layout.inodes_per_group,
			magic: EXT4_MAGIC,
			inode_size: 256,
			first_ino: layout.first_ino,
			feature_incompat: INCOMPAT_EXTENTS,
			feature_compat: 0,
			desc_size: 32,
		}
	}
}
