/*
 * ext2/superblock.rs - ext2 Superblock Parser
 *
 * Reads and validates the ext2 superblock from sector 2 (byte 1024).
 * Provides derived geometry values used by all other ext2 submodules.
 */

use crate::BlockDev;

pub const EXT2_MAGIC: u16 = 0xEF53;

/* Superblock lives at byte offset 1024 = sector 2 */
const SUPERBLOCK_SECTOR: u64 = 2;

/* ------------------------------------------------------------------ */
/*  On-disk superblock layout (1024 bytes, fields we care about)       */
/* ------------------------------------------------------------------ */

/*
 * Only the fields we actually use are extracted; the rest of the 1024-byte
 * structure is consumed as raw bytes and discarded.
 */
#[allow(dead_code)]
pub struct Superblock {
	pub inodes_count:     u32,
	pub blocks_count:     u32,
	pub free_blocks:      u32,
	pub free_inodes:      u32,
	pub first_data_block: u32,   /* 1 for 1 KiB blocks, else 0 */
	pub log_block_size:   u32,   /* block_size = 1024 << log_block_size */
	pub blocks_per_group: u32,
	pub inodes_per_group: u32,
	pub magic:            u16,   /* 0xEF53 */
	pub inode_size:       u16,   /* 128 for ext2 rev 0, ≥128 for rev 1 */
	pub first_ino:        u32,   /* first non-reserved inode (usually 11) */
}

impl Superblock {
	/*
	 * read - Parse the superblock from the given block device.
	 *
	 * Returns None if the device is unreadable or the magic is wrong.
	 */
	pub fn read(dev: &dyn BlockDev) -> Option<Self> {
		/* Superblock spans bytes 1024–2047, which is sectors 2 and 3 */
		let mut s2 = [0u8; 512];
		let mut s3 = [0u8; 512];
		if !dev.read_block(SUPERBLOCK_SECTOR, &mut s2) { return None; }
		if !dev.read_block(SUPERBLOCK_SECTOR + 1, &mut s3) { return None; }

		/* Concatenate into a 1024-byte view */
		let mut raw = [0u8; 1024];
		raw[..512].copy_from_slice(&s2);
		raw[512..].copy_from_slice(&s3);

		let magic = u16::from_le_bytes([raw[56], raw[57]]);
		if magic != EXT2_MAGIC { return None; }

		let inodes_count     = u32::from_le_bytes(raw[0..4].try_into().ok()?);
		let blocks_count     = u32::from_le_bytes(raw[4..8].try_into().ok()?);
		let free_blocks      = u32::from_le_bytes(raw[12..16].try_into().ok()?);
		let free_inodes      = u32::from_le_bytes(raw[16..20].try_into().ok()?);
		let first_data_block = u32::from_le_bytes(raw[20..24].try_into().ok()?);
		let log_block_size   = u32::from_le_bytes(raw[24..28].try_into().ok()?);
		let blocks_per_group = u32::from_le_bytes(raw[32..36].try_into().ok()?);
		let inodes_per_group = u32::from_le_bytes(raw[40..44].try_into().ok()?);
		let inode_size       = u16::from_le_bytes(raw[88..90].try_into().ok()?);
		let first_ino        = u32::from_le_bytes(raw[84..88].try_into().ok()?);

		Some(Superblock {
			inodes_count,
			blocks_count,
			free_blocks,
			free_inodes,
			first_data_block,
			log_block_size,
			blocks_per_group,
			inodes_per_group,
			magic,
			inode_size,
			first_ino,
		})
	}

	/*
	 * write_free_counts - Flush free_blocks and free_inodes back to disk.
	 *
	 * Called after alloc/free operations update the in-memory counts.
	 */
	pub fn write_free_counts(&self, dev: &dyn BlockDev) {
		let mut s2 = [0u8; 512];
		let mut s3 = [0u8; 512];
		if !dev.read_block(SUPERBLOCK_SECTOR, &mut s2) { return; }
		if !dev.read_block(SUPERBLOCK_SECTOR + 1, &mut s3) { return; }

		let mut raw = [0u8; 1024];
		raw[..512].copy_from_slice(&s2);
		raw[512..].copy_from_slice(&s3);

		raw[12..16].copy_from_slice(&self.free_blocks.to_le_bytes());
		raw[16..20].copy_from_slice(&self.free_inodes.to_le_bytes());

		s2.copy_from_slice(&raw[..512]);
		s3.copy_from_slice(&raw[512..]);
		dev.write_block(SUPERBLOCK_SECTOR, &s2);
		dev.write_block(SUPERBLOCK_SECTOR + 1, &s3);
	}

	/* block_size - Bytes per block */
	pub fn block_size(&self) -> usize {
		1024 << self.log_block_size
	}

	/* sectors_per_block - How many 512-byte sectors fit in one block */
	pub fn sectors_per_block(&self) -> u64 {
		(self.block_size() / 512) as u64
	}

	/* block_to_sector - Convert logical block number to first sector */
	pub fn block_to_sector(&self, block: u64) -> u64 {
		block * self.sectors_per_block()
	}

	/* num_block_groups - Total number of block groups */
	pub fn num_block_groups(&self) -> usize {
		let n = (self.blocks_count as usize + self.blocks_per_group as usize - 1)
			/ self.blocks_per_group as usize;
		core::cmp::max(n, 1)
	}

	/*
	 * bgdt_block - Block number where the BGDT starts.
	 *
	 * For 1 KiB blocks: block 2 (superblock is block 1).
	 * For larger blocks: block 1 (superblock fits inside block 0).
	 */
	pub fn bgdt_block(&self) -> u64 {
		(self.first_data_block + 1) as u64
	}

	/* inode_block_group - Which block group owns inode `ino` (1-based) */
	pub fn inode_block_group(&self, ino: u32) -> u32 {
		(ino - 1) / self.inodes_per_group
	}

	/* inode_local_index - 0-based index within the group's inode table */
	pub fn inode_local_index(&self, ino: u32) -> u32 {
		(ino - 1) % self.inodes_per_group
	}
}
