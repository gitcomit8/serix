/*
 * ext4/superblock.rs - ext4 Superblock Parser
 *
 * Reads the 1024-byte superblock from sector 2 (byte offset 1024).
 * Identical magic to ext2/ext3 (0xEF53); distinguished by feature flags.
 */

use crate::BlockDev;

pub const EXT4_MAGIC: u16 = 0xEF53;
/* Incompatible features we must understand to mount read-write */
pub const INCOMPAT_EXTENTS:	u32 = 0x0040;
pub const INCOMPAT_64BIT:	u32 = 0x0080; /* we reject this */
pub const INCOMPAT_FLEX_BG:	u32 = 0x0200; /* harmless for our use */

pub struct Superblock {
	pub inodes_count:	u32,
	pub blocks_count_lo:	u32,
	pub free_blocks_lo:	u32,
	pub free_inodes:	u32,
	pub first_data_block:	u32,
	pub log_block_size:	u32,
	pub blocks_per_group:	u32,
	pub inodes_per_group:	u32,
	pub magic:		u16,
	pub inode_size:		u16,
	pub first_ino:		u32,
	pub feature_incompat:	u32,
	pub feature_compat:	u32,
	pub desc_size:		u16,  /* size of each BGDT entry (32 or 64 bytes) */
}

impl Superblock {
	pub fn read(dev: &dyn BlockDev) -> Option<Self> {
		let mut s2 = [0u8; 512];
		let mut s3 = [0u8; 512];
		if !dev.read_block(2, &mut s2) { return None; }
		if !dev.read_block(3, &mut s3) { return None; }
		let mut raw = [0u8; 1024];
		raw[..512].copy_from_slice(&s2);
		raw[512..].copy_from_slice(&s3);

		let magic = u16::from_le_bytes([raw[56], raw[57]]);
		if magic != EXT4_MAGIC { return None; }

		let feature_incompat = u32::from_le_bytes(raw[96..100].try_into().ok()?);
		/* Reject 64-bit addressing — we only handle 32-bit block numbers */
		if feature_incompat & INCOMPAT_64BIT != 0 { return None; }

		let desc_size = u16::from_le_bytes([raw[254], raw[255]]);
		let desc_size = if desc_size < 32 { 32 } else { desc_size };

		Some(Superblock {
			inodes_count:     u32::from_le_bytes(raw[0..4].try_into().ok()?),
			blocks_count_lo:  u32::from_le_bytes(raw[4..8].try_into().ok()?),
			free_blocks_lo:   u32::from_le_bytes(raw[12..16].try_into().ok()?),
			free_inodes:      u32::from_le_bytes(raw[16..20].try_into().ok()?),
			first_data_block: u32::from_le_bytes(raw[20..24].try_into().ok()?),
			log_block_size:   u32::from_le_bytes(raw[24..28].try_into().ok()?),
			blocks_per_group: u32::from_le_bytes(raw[32..36].try_into().ok()?),
			inodes_per_group: u32::from_le_bytes(raw[40..44].try_into().ok()?),
			magic,
			inode_size:       u16::from_le_bytes(raw[88..90].try_into().ok()?),
			first_ino:        u32::from_le_bytes(raw[84..88].try_into().ok()?),
			feature_incompat,
			feature_compat:   u32::from_le_bytes(raw[92..96].try_into().ok()?),
			desc_size,
		})
	}

	pub fn block_size(&self) -> usize       { 1024 << self.log_block_size }
	pub fn sectors_per_block(&self) -> u64  { (self.block_size() / 512) as u64 }
	pub fn block_to_sector(&self, b: u64) -> u64 { b * self.sectors_per_block() }

	pub fn num_block_groups(&self) -> usize {
		let n = (self.blocks_count_lo as usize + self.blocks_per_group as usize - 1)
			/ self.blocks_per_group as usize;
		core::cmp::max(n, 1)
	}

	pub fn bgdt_block(&self) -> u64 { (self.first_data_block + 1) as u64 }

	pub fn inode_block_group(&self, ino: u32) -> u32 { (ino - 1) / self.inodes_per_group }
	pub fn inode_local_index(&self, ino: u32) -> u32  { (ino - 1) % self.inodes_per_group }

	pub fn write_free_counts(&self, dev: &dyn BlockDev) {
		let mut s2 = [0u8; 512]; let mut s3 = [0u8; 512];
		if !dev.read_block(2, &mut s2) { return; }
		if !dev.read_block(3, &mut s3) { return; }
		let mut raw = [0u8; 1024];
		raw[..512].copy_from_slice(&s2);
		raw[512..].copy_from_slice(&s3);
		raw[12..16].copy_from_slice(&self.free_blocks_lo.to_le_bytes());
		raw[16..20].copy_from_slice(&self.free_inodes.to_le_bytes());
		s2.copy_from_slice(&raw[..512]);
		s3.copy_from_slice(&raw[512..]);
		dev.write_block(2, &s2);
		dev.write_block(3, &s3);
	}
}
