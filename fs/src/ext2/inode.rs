/*
 * ext2/inode.rs - ext2 Inode Read/Write and Block Map Resolution
 *
 * Handles on-disk inode I/O and translates logical block indices to
 * physical block numbers through direct, single-indirect, double-indirect,
 * and triple-indirect block maps.
 */

extern crate alloc;
use alloc::vec::Vec;
use crate::BlockDev;
use super::superblock::Superblock;
use super::bgdt::BgDescTable;

/* ------------------------------------------------------------------ */
/*  On-disk inode (128 bytes for ext2 rev 0)                          */
/* ------------------------------------------------------------------ */

pub const EXT2_S_IFREG: u16 = 0x8000;
pub const EXT2_S_IFDIR: u16 = 0x4000;

pub struct Inode {
	pub mode:        u16,
	pub links_count: u16,  /* must be ≥1 for allocated inodes (e2fsck) */
	pub size:        u32,
	pub blocks:      u32,  /* 512-byte units */
	pub block:       [u32; 15],
	pub ino:         u32,
}

impl Inode {
	/* is_dir / is_file helpers */
	pub fn is_dir(&self)  -> bool { self.mode & 0xF000 == EXT2_S_IFDIR }
	pub fn is_file(&self) -> bool { self.mode & 0xF000 == EXT2_S_IFREG }
	pub fn size(&self)    -> usize { self.size as usize }
}

/* ------------------------------------------------------------------ */
/*  Disk I/O helpers                                                   */
/* ------------------------------------------------------------------ */

/*
 * read_block_bytes - Read one full filesystem block into a Vec<u8>.
 */
fn read_block_bytes(dev: &dyn BlockDev, sb: &Superblock, block: u32) -> Vec<u8> {
	let bsz = sb.block_size();
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(block as u64);
	let mut out = Vec::with_capacity(bsz);
	for s in 0..spb {
		let mut buf = [0u8; 512];
		dev.read_block(sec + s, &mut buf);
		out.extend_from_slice(&buf);
	}
	out
}

/*
 * write_block_bytes - Write one full filesystem block from a slice.
 */
fn write_block_bytes(dev: &dyn BlockDev, sb: &Superblock, block: u32, data: &[u8]) {
	let spb = sb.sectors_per_block();
	let sec = sb.block_to_sector(block as u64);
	for s in 0..spb as usize {
		let mut buf = [0u8; 512];
		buf.copy_from_slice(&data[s * 512..(s + 1) * 512]);
		dev.write_block(sec + s as u64, &buf);
	}
}

/* ------------------------------------------------------------------ */
/*  Inode read / write                                                 */
/* ------------------------------------------------------------------ */

impl Inode {
	/*
	 * read - Load inode `ino` from the inode table.
	 *
	 * Inode numbers are 1-based. The on-disk record is `inode_size` bytes
	 * wide (128 for ext2 rev 0); we parse only the first 128 bytes.
	 */
	pub fn read(dev: &dyn BlockDev, sb: &Superblock, bgdt: &BgDescTable, ino: u32) -> Option<Self> {
		let group    = sb.inode_block_group(ino) as usize;
		let idx      = sb.inode_local_index(ino) as usize;
		let isz      = sb.inode_size as usize;
		let bsz      = sb.block_size();

		let bg        = bgdt.get(group);
		let table_blk = bg.inode_table as u64;

		/* Byte offset of this inode within the inode table */
		let byte_off   = idx * isz;
		let block_idx  = byte_off / bsz;
		let off_in_blk = byte_off % bsz;

		let blk_data = read_block_bytes(dev, sb, (table_blk + block_idx as u64) as u32);
		if blk_data.len() < off_in_blk + 128 { return None; }

		let r = &blk_data[off_in_blk..off_in_blk + 128];
		let mode        = u16::from_le_bytes([r[0],  r[1]]);
		let links_count = u16::from_le_bytes([r[26], r[27]]);
		let size        = u32::from_le_bytes([r[4],  r[5],  r[6],  r[7]]);
		let blocks      = u32::from_le_bytes([r[28], r[29], r[30], r[31]]);

		let mut block = [0u32; 15];
		for i in 0..15 {
			let base = 40 + i * 4;
			block[i] = u32::from_le_bytes([r[base], r[base+1], r[base+2], r[base+3]]);
		}

		Some(Inode { mode, links_count, size, blocks, block, ino })
	}

	/*
	 * write - Flush this inode back to disk.
	 */
	pub fn write(&self, dev: &dyn BlockDev, sb: &Superblock, bgdt: &BgDescTable) {
		let group    = sb.inode_block_group(self.ino) as usize;
		let idx      = sb.inode_local_index(self.ino) as usize;
		let isz      = sb.inode_size as usize;
		let bsz      = sb.block_size();

		let bg        = bgdt.get(group);
		let table_blk = bg.inode_table as u64;

		let byte_off   = idx * isz;
		let block_idx  = byte_off / bsz;
		let off_in_blk = byte_off % bsz;

		let mut blk_data = read_block_bytes(dev, sb, (table_blk + block_idx as u64) as u32);
		let dst = &mut blk_data[off_in_blk..off_in_blk + 128];

		dst[0..2].copy_from_slice(&self.mode.to_le_bytes());
		dst[26..28].copy_from_slice(&self.links_count.to_le_bytes());
		dst[4..8].copy_from_slice(&self.size.to_le_bytes());
		dst[28..32].copy_from_slice(&self.blocks.to_le_bytes());
		for i in 0..15 {
			let base = 40 + i * 4;
			dst[base..base+4].copy_from_slice(&self.block[i].to_le_bytes());
		}

		write_block_bytes(dev, sb, (table_blk + block_idx as u64) as u32, &blk_data);
	}

	/* ---------------------------------------------------------------- */
	/*  Block map resolution                                             */
	/* ---------------------------------------------------------------- */

	/*
	 * get_block - Resolve logical block index `n` to a physical block number.
	 *
	 * Returns None if the block has not been allocated (sparse file / EOF).
	 */
	pub fn get_block(&self, dev: &dyn BlockDev, sb: &Superblock, n: u32) -> Option<u32> {
		let bsz     = sb.block_size();
		let ptrs    = (bsz / 4) as u32;   /* indirect block capacity */

		/* Direct blocks: [0..11] */
		if n < 12 {
			let b = self.block[n as usize];
			return if b == 0 { None } else { Some(b) };
		}

		/* Single indirect: block[12] */
		let n1 = n - 12;
		if n1 < ptrs {
			let ib = self.block[12];
			if ib == 0 { return None; }
			let data = read_block_bytes(dev, sb, ib);
			let b = u32_at(&data, n1 as usize);
			return if b == 0 { None } else { Some(b) };
		}

		/* Double indirect: block[13] */
		let n2 = n1 - ptrs;
		if n2 < ptrs * ptrs {
			let dib = self.block[13];
			if dib == 0 { return None; }
			let data1 = read_block_bytes(dev, sb, dib);
			let ib = u32_at(&data1, (n2 / ptrs) as usize);
			if ib == 0 { return None; }
			let data2 = read_block_bytes(dev, sb, ib);
			let b = u32_at(&data2, (n2 % ptrs) as usize);
			return if b == 0 { None } else { Some(b) };
		}

		/* Triple indirect: block[14] */
		let n3 = n2 - ptrs * ptrs;
		let tib = self.block[14];
		if tib == 0 { return None; }
		let data1 = read_block_bytes(dev, sb, tib);
		let ib2 = u32_at(&data1, (n3 / (ptrs * ptrs)) as usize);
		if ib2 == 0 { return None; }
		let data2 = read_block_bytes(dev, sb, ib2);
		let ib = u32_at(&data2, ((n3 / ptrs) % ptrs) as usize);
		if ib == 0 { return None; }
		let data3 = read_block_bytes(dev, sb, ib);
		let b = u32_at(&data3, (n3 % ptrs) as usize);
		if b == 0 { None } else { Some(b) }
	}

	/*
	 * set_block - Store a physical block number for logical index `n`.
	 *
	 * Allocates indirect blocks from the allocator as needed.
	 * `phys` must already be allocated by alloc::alloc_block.
	 */
	pub fn set_block(
		&mut self,
		dev: &dyn BlockDev,
		sb: &mut Superblock,
		bgdt: &mut BgDescTable,
		n: u32,
		phys: u32,
	) {
		let bsz  = sb.block_size();
		let ptrs = (bsz / 4) as u32;

		/* Direct */
		if n < 12 {
			self.block[n as usize] = phys;
			return;
		}

		let n1 = n - 12;

		/* Single indirect */
		if n1 < ptrs {
			if self.block[12] == 0 {
				let ib = super::bitmap_alloc::alloc_block(dev, sb, bgdt).unwrap_or(0);
				self.block[12] = ib;
				/* zero-fill new indirect block */
				let zeros = alloc::vec![0u8; bsz];
				write_block_bytes(dev, sb, ib, &zeros);
			}
			let ib = self.block[12];
			let mut data = read_block_bytes(dev, sb, ib);
			write_u32_at(&mut data, n1 as usize, phys);
			write_block_bytes(dev, sb, ib, &data);
			return;
		}

		let n2 = n1 - ptrs;

		/* Double indirect */
		if n2 < ptrs * ptrs {
			if self.block[13] == 0 {
				let dib = super::bitmap_alloc::alloc_block(dev, sb, bgdt).unwrap_or(0);
				self.block[13] = dib;
				let zeros = alloc::vec![0u8; bsz];
				write_block_bytes(dev, sb, dib, &zeros);
			}
			let dib = self.block[13];
			let mut data1 = read_block_bytes(dev, sb, dib);
			let ib_idx = (n2 / ptrs) as usize;
			let mut ib = u32_at(&data1, ib_idx);
			if ib == 0 {
				ib = super::bitmap_alloc::alloc_block(dev, sb, bgdt).unwrap_or(0);
				write_u32_at(&mut data1, ib_idx, ib);
				write_block_bytes(dev, sb, dib, &data1);
				let zeros = alloc::vec![0u8; bsz];
				write_block_bytes(dev, sb, ib, &zeros);
			}
			let mut data2 = read_block_bytes(dev, sb, ib);
			write_u32_at(&mut data2, (n2 % ptrs) as usize, phys);
			write_block_bytes(dev, sb, ib, &data2);
			return;
		}

		/* Triple indirect (rare; minimal implementation) */
		let n3 = n2 - ptrs * ptrs;
		if self.block[14] == 0 {
			let tib = super::bitmap_alloc::alloc_block(dev, sb, bgdt).unwrap_or(0);
			self.block[14] = tib;
			let zeros = alloc::vec![0u8; bsz];
			write_block_bytes(dev, sb, tib, &zeros);
		}
		let tib = self.block[14];
		let mut data1 = read_block_bytes(dev, sb, tib);
		let ib2_idx = (n3 / (ptrs * ptrs)) as usize;
		let mut ib2 = u32_at(&data1, ib2_idx);
		if ib2 == 0 {
			ib2 = super::bitmap_alloc::alloc_block(dev, sb, bgdt).unwrap_or(0);
			write_u32_at(&mut data1, ib2_idx, ib2);
			write_block_bytes(dev, sb, tib, &data1);
			let zeros = alloc::vec![0u8; bsz];
			write_block_bytes(dev, sb, ib2, &zeros);
		}
		let mut data2 = read_block_bytes(dev, sb, ib2);
		let ib_idx = ((n3 / ptrs) % ptrs) as usize;
		let mut ib = u32_at(&data2, ib_idx);
		if ib == 0 {
			ib = super::bitmap_alloc::alloc_block(dev, sb, bgdt).unwrap_or(0);
			write_u32_at(&mut data2, ib_idx, ib);
			write_block_bytes(dev, sb, ib2, &data2);
			let zeros = alloc::vec![0u8; bsz];
			write_block_bytes(dev, sb, ib, &zeros);
		}
		let mut data3 = read_block_bytes(dev, sb, ib);
		write_u32_at(&mut data3, (n3 % ptrs) as usize, phys);
		write_block_bytes(dev, sb, ib, &data3);
	}
}

/* ------------------------------------------------------------------ */
/*  Little-endian u32 access helpers                                   */
/* ------------------------------------------------------------------ */

fn u32_at(data: &[u8], idx: usize) -> u32 {
	let off = idx * 4;
	u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]])
}

fn write_u32_at(data: &mut [u8], idx: usize, val: u32) {
	let off = idx * 4;
	data[off..off+4].copy_from_slice(&val.to_le_bytes());
}
