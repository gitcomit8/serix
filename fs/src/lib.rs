/*
 * fs/src/lib.rs - Minimal FAT32 Filesystem Driver
 *
 * Implements FAT32 read/write over the VirtIO block device without
 * external crate dependencies. Supports:
 *  - BPB parsing and filesystem layout discovery
 *  - FAT cluster chain traversal and allocation
 *  - Directory entry reading (8.3 and LFN names)
 *  - Directory entry creation (8.3 + LFN pair)
 *  - File read/write with seek
 *
 * The disk.img is formatted with mkfs.vfat -F 32 (see Makefile).
 * On Linux: sudo mount -o loop disk.img /mnt
 */

#![no_std]
extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, Once};
use vfs::{FileType, INode};

/* ------------------------------------------------------------------ */
/*  FAT32 Constants                                                     */
/* ------------------------------------------------------------------ */

const SECTOR_SIZE: usize = 512;
const FAT32_EOC: u32 = 0x0FFF_FFF8;   /* End-of-chain threshold      */
const FAT32_FREE: u32 = 0;
const FAT32_MASK: u32 = 0x0FFF_FFFF;  /* Valid bits in a FAT entry   */
const DIR_ENTRY_SIZE: usize = 32;
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LFN: u8 = ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID;

/* ------------------------------------------------------------------ */
/*  Timestamps                                                          */
/* ------------------------------------------------------------------ */

/*
 * fat32_timestamp - Encode current LAPIC ticks as FAT32 time/date words
 *
 * Returns (time_word, date_word) for use in directory entries.
 * Date is fixed at 1980-01-01 (no RTC); time is derived from boot ticks.
 */
fn fat32_timestamp() -> (u16, u16) {
	let ticks = apic::timer::ticks();
	let secs = ticks / 625;
	let hours = (secs / 3600) % 24;
	let minutes = (secs / 60) % 60;
	let two_secs = (secs % 60) / 2;
	let time_word = ((hours << 11) | (minutes << 5) | two_secs) as u16;
	/* Date: year=0 (1980), month=1, day=1 */
	let date_word: u16 = (0 << 9) | (1 << 5) | 1;
	(time_word, date_word)
}

/* ------------------------------------------------------------------ */
/*  BPB — BIOS Parameter Block                                         */
/* ------------------------------------------------------------------ */

/*
 * struct Bpb - Parsed FAT32 BPB parameters needed for I/O
 */
#[derive(Debug, Clone, Copy)]
struct Bpb {
	bytes_per_sector: u16,
	sectors_per_cluster: u8,
	reserved_sectors: u16,
	fat_count: u8,
	total_sectors: u32,
	sectors_per_fat: u32,
	root_cluster: u32,
}

impl Bpb {
	/*
	 * parse - Parse BPB from raw sector 0 bytes
	 */
	fn parse(buf: &[u8; SECTOR_SIZE]) -> Option<Self> {
		/* Boot sector signature */
		if buf[510] != 0x55 || buf[511] != 0xAA {
			return None;
		}

		let bytes_per_sector = u16::from_le_bytes([buf[11], buf[12]]);
		let sectors_per_cluster = buf[13];
		let reserved_sectors = u16::from_le_bytes([buf[14], buf[15]]);
		let fat_count = buf[16];
		/* bytes 17-19: root_entry_count (0 for FAT32) */
		let total_sectors_16 = u16::from_le_bytes([buf[19], buf[20]]);
		/* byte 21: media type */
		let sectors_per_fat_16 = u16::from_le_bytes([buf[22], buf[23]]);
		/* bytes 24-27: geometry */
		/* bytes 28-31: hidden sectors */
		let total_sectors_32 = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);

		/* FAT32-specific fields at offset 36 */
		let sectors_per_fat_32 =
			u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);
		/* bytes 40-43: ext flags, fs ver */
		let root_cluster =
			u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]);

		let sectors_per_fat = if sectors_per_fat_16 == 0 {
			sectors_per_fat_32
		} else {
			sectors_per_fat_16 as u32
		};

		let total_sectors = if total_sectors_16 == 0 {
			total_sectors_32
		} else {
			total_sectors_16 as u32
		};

		if bytes_per_sector == 0
			|| sectors_per_cluster == 0
			|| fat_count == 0
			|| sectors_per_fat == 0
		{
			return None;
		}

		Some(Bpb {
			bytes_per_sector,
			sectors_per_cluster,
			reserved_sectors,
			fat_count,
			total_sectors,
			sectors_per_fat,
			root_cluster,
		})
	}

	/* fat_start_sector - First sector of FAT 0 */
	fn fat_start_sector(&self) -> u64 {
		self.reserved_sectors as u64
	}

	/* data_start_sector - First sector of the data region */
	fn data_start_sector(&self) -> u64 {
		self.reserved_sectors as u64
			+ self.fat_count as u64 * self.sectors_per_fat as u64
	}

	/* cluster_to_sector - Convert cluster number to first sector */
	fn cluster_to_sector(&self, cluster: u32) -> u64 {
		self.data_start_sector()
			+ (cluster as u64 - 2) * self.sectors_per_cluster as u64
	}

	/* cluster_size_bytes - Bytes per cluster */
	fn cluster_size_bytes(&self) -> usize {
		self.sectors_per_cluster as usize * SECTOR_SIZE
	}

	/* fat_sector_for - Sector containing the FAT entry for cluster */
	fn fat_sector_for(&self, cluster: u32) -> u64 {
		self.fat_start_sector() + (cluster as u64 * 4) / SECTOR_SIZE as u64
	}

	/* fat_offset_in_sector - Byte offset of FAT entry within its sector */
	fn fat_offset_in_sector(&self, cluster: u32) -> usize {
		(cluster as usize * 4) % SECTOR_SIZE
	}
}

/* ------------------------------------------------------------------ */
/*  Raw I/O helpers                                                     */
/* ------------------------------------------------------------------ */

/*
 * read_sector - Read a sector from VirtIO block device
 */
fn read_sector(sector: u64, buf: &mut [u8; SECTOR_SIZE]) {
	if let Some(blk) = drivers::virtio::virtio_blk() {
		let _ = blk.lock().read_sector(sector, buf);
	}
}

/*
 * write_sector - Write a sector to VirtIO block device
 */
fn write_sector(sector: u64, buf: &[u8; SECTOR_SIZE]) {
	if let Some(blk) = drivers::virtio::virtio_blk() {
		let _ = blk.lock().write_sector(sector, buf);
	}
}

/* ------------------------------------------------------------------ */
/*  FAT Cluster Chain Operations                                        */
/* ------------------------------------------------------------------ */

/*
 * fat_read_entry - Read the FAT32 entry for a cluster
 *
 * Returns the next cluster number (or EOC/free marker).
 */
fn fat_read_entry(bpb: &Bpb, cluster: u32) -> u32 {
	let sector = bpb.fat_sector_for(cluster);
	let offset = bpb.fat_offset_in_sector(cluster);
	let mut buf = [0u8; SECTOR_SIZE];
	read_sector(sector, &mut buf);
	u32::from_le_bytes([
		buf[offset],
		buf[offset + 1],
		buf[offset + 2],
		buf[offset + 3],
	]) & FAT32_MASK
}

/*
 * fat_write_entry - Write the FAT32 entry for a cluster (all FAT copies)
 */
fn fat_write_entry(bpb: &Bpb, cluster: u32, value: u32) {
	for fat_num in 0..bpb.fat_count as u64 {
		let fat_base =
			bpb.reserved_sectors as u64 + fat_num * bpb.sectors_per_fat as u64;
		let sector = fat_base + (cluster as u64 * 4) / SECTOR_SIZE as u64;
		let offset = (cluster as usize * 4) % SECTOR_SIZE;
		let mut buf = [0u8; SECTOR_SIZE];
		read_sector(sector, &mut buf);
		let bytes = (value & FAT32_MASK).to_le_bytes();
		buf[offset] = bytes[0];
		buf[offset + 1] = bytes[1];
		buf[offset + 2] = bytes[2];
		/* Preserve upper nibble of byte 3 */
		buf[offset + 3] = (buf[offset + 3] & 0xF0) | bytes[3];
		write_sector(sector, &buf);
	}
}

/*
 * fat_alloc_cluster - Find a free cluster, mark it EOC, return it
 *
 * Scans the FAT from cluster 2 onward for a free entry.
 */
fn fat_alloc_cluster(bpb: &Bpb) -> Option<u32> {
	let max_cluster =
		(bpb.total_sectors - bpb.data_start_sector() as u32)
			/ bpb.sectors_per_cluster as u32
			+ 2;

	for cluster in 2..max_cluster {
		if fat_read_entry(bpb, cluster) == FAT32_FREE {
			fat_write_entry(bpb, cluster, FAT32_EOC);
			return Some(cluster);
		}
	}
	None
}

/*
 * cluster_chain - Collect all clusters in a chain starting at first_cluster
 */
fn cluster_chain(bpb: &Bpb, first_cluster: u32) -> Vec<u32> {
	let mut chain = Vec::new();
	let mut cur = first_cluster;
	while cur >= 2 && cur < FAT32_EOC {
		chain.push(cur);
		let next = fat_read_entry(bpb, cur);
		if next >= FAT32_EOC || next < 2 {
			break;
		}
		cur = next;
	}
	chain
}

/* ------------------------------------------------------------------ */
/*  Directory Entry Parsing                                             */
/* ------------------------------------------------------------------ */

/*
 * struct DirEntry - Parsed directory entry
 */
#[derive(Debug, Clone)]
struct DirEntry {
	name: String,        /* Full name (LFN if available, else 8.3) */
	attr: u8,            /* Attributes byte                        */
	first_cluster: u32,  /* First data cluster                     */
	size: u32,           /* File size in bytes (0 for directories) */
	/* Offset of the 8.3 entry within its sector, for updates */
	entry_sector: u64,
	entry_offset: usize,
	/* Start of the LFN chain preceding this entry (equals entry_sector/
	 * entry_offset when no LFN is present) */
	lfn_start_sector: u64,
	lfn_start_offset: usize,
}

impl DirEntry {
	fn is_dir(&self) -> bool {
		self.attr & ATTR_DIRECTORY != 0
	}
}

/*
 * read_lfn_chars - Extract up to 13 UCS-2 characters from an LFN entry
 */
fn read_lfn_chars(raw: &[u8; 32]) -> [u16; 13] {
	let mut chars = [0u16; 13];
	/* Positions of UCS-2 chars within an LFN entry */
	let offsets: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
	for (i, &off) in offsets.iter().enumerate() {
		chars[i] = u16::from_le_bytes([raw[off], raw[off + 1]]);
	}
	chars
}

/*
 * parse_sfn - Parse short file name (8.3) into a String
 *
 * Strips trailing spaces, inserts '.' between name and extension.
 */
fn parse_sfn(raw: &[u8; 11]) -> String {
	let name = core::str::from_utf8(&raw[0..8])
		.unwrap_or("")
		.trim_end_matches(' ');
	let ext = core::str::from_utf8(&raw[8..11])
		.unwrap_or("")
		.trim_end_matches(' ');
	if ext.is_empty() {
		name.to_string()
	} else {
		alloc::format!("{}.{}", name, ext)
	}
}

/*
 * build_sfn - Convert a filename to FAT32 8.3 short name bytes
 *
 * Returns 11 bytes: 8 name + 3 extension, space-padded, uppercase.
 * Returns None if the name cannot be represented.
 */
fn build_sfn(name: &str) -> Option<[u8; 11]> {
	let mut sfn = [b' '; 11];
	let (base, ext) = if let Some(dot) = name.rfind('.') {
		(&name[..dot], &name[dot + 1..])
	} else {
		(name, "")
	};

	if base.is_empty() || base.len() > 8 || ext.len() > 3 {
		return None;
	}
	/* Check for characters invalid in SFN */
	let invalid = |c: char| {
		matches!(c, ' ' | '.' | '"' | '*' | '+' | ',' | '/' | ':' | ';' | '<' | '='
			| '>' | '?' | '[' | '\\' | ']' | '|')
	};
	if base.chars().any(invalid) || ext.chars().any(invalid) {
		return None;
	}

	for (i, b) in base.bytes().enumerate() {
		sfn[i] = b.to_ascii_uppercase();
	}
	for (i, b) in ext.bytes().enumerate() {
		sfn[8 + i] = b.to_ascii_uppercase();
	}
	Some(sfn)
}

/*
 * sfn_checksum - Compute the LFN checksum of an 8.3 short name
 */
fn sfn_checksum(sfn: &[u8; 11]) -> u8 {
	let mut sum: u8 = 0;
	for &b in sfn.iter() {
		sum = (sum >> 1).wrapping_add(sum << 7).wrapping_add(b);
	}
	sum
}

/*
 * read_dir_entries - Parse all valid directory entries from a cluster chain
 *
 * Handles LFN entries by accumulating UCS-2 character sequences before
 * the corresponding 8.3 entry.
 */
fn read_dir_entries(bpb: &Bpb, first_cluster: u32) -> Vec<DirEntry> {
	let mut entries = Vec::new();
	let chain = cluster_chain(bpb, first_cluster);
	let cluster_size = bpb.cluster_size_bytes();
	let mut lfn_buf: Vec<u16> = Vec::new();
	let mut lfn_seq: u8 = 0;
	let mut lfn_start: Option<(u64, usize)> = None;

	for cluster in chain {
		let start_sector = bpb.cluster_to_sector(cluster);
		for s in 0..bpb.sectors_per_cluster as u64 {
			let mut sector_buf = [0u8; SECTOR_SIZE];
			let sector = start_sector + s;
			read_sector(sector, &mut sector_buf);

			for entry_idx in 0..(SECTOR_SIZE / DIR_ENTRY_SIZE) {
				let offset = entry_idx * DIR_ENTRY_SIZE;
				let raw: &[u8; 32] = sector_buf[offset..offset + 32]
					.try_into()
					.unwrap();

				/* 0x00 = end of directory */
				if raw[0] == 0x00 {
					return entries;
				}
				/* 0xE5 = deleted entry */
				if raw[0] == 0xE5 {
					lfn_buf.clear();
					lfn_start = None;
					continue;
				}
				/* LFN entry */
				if raw[11] == ATTR_LFN {
					let seq = raw[0] & 0x3F;
					let is_last = raw[0] & 0x40 != 0;
					if is_last {
						lfn_buf.clear();
						lfn_seq = seq;
						lfn_start = Some((sector, offset));
					}
					/* Prepend chars (LFN entries come in reverse order) */
					let chars = read_lfn_chars(raw);
					let mut chunk: Vec<u16> = chars
						.iter()
						.take_while(|&&c| c != 0)
						.copied()
						.collect();
					let mut combined = chunk;
					combined.extend_from_slice(&lfn_buf);
					lfn_buf = combined;
					continue;
				}

				/* Skip volume label */
				if raw[11] & ATTR_VOLUME_ID != 0
					&& raw[11] & ATTR_DIRECTORY == 0
				{
					lfn_buf.clear();
					lfn_start = None;
					continue;
				}

				/* 8.3 entry */
				let sfn_bytes: [u8; 11] = raw[0..11].try_into().unwrap();
				let sfn_name = parse_sfn(&sfn_bytes);

				/* Build final name from LFN or 8.3 */
				let name = if !lfn_buf.is_empty() {
					/* Decode UCS-2 to String, stopping at null */
					let s: String = lfn_buf
						.iter()
						.take_while(|&&c| c != 0)
						.filter_map(|&c| char::from_u32(c as u32))
						.collect();
					lfn_buf.clear();
					s
				} else {
					sfn_name
				};

				let first_cluster_hi =
					u16::from_le_bytes([raw[20], raw[21]]) as u32;
				let first_cluster_lo =
					u16::from_le_bytes([raw[26], raw[27]]) as u32;
				let first_cluster = (first_cluster_hi << 16) | first_cluster_lo;
				let size = u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]);
				let attr = raw[11];

				/* Skip dot entries */
				if name == "." || name == ".." {
					lfn_start = None;
					continue;
				}

				let (ls, lo) = lfn_start.unwrap_or((sector, offset));
				entries.push(DirEntry {
					name,
					attr,
					first_cluster,
					size,
					entry_sector: sector,
					entry_offset: offset,
					lfn_start_sector: ls,
					lfn_start_offset: lo,
				});
				lfn_start = None;
			}
		}
	}
	entries
}

/* ------------------------------------------------------------------ */
/*  File Read / Write                                                   */
/* ------------------------------------------------------------------ */

/*
 * file_read - Read bytes from a file at a byte offset
 *
 * @cluster: First cluster of the file
 * @file_size: File size in bytes
 * @offset: Byte offset to start reading from
 * @buf: Destination buffer
 *
 * Return: Number of bytes read
 */
fn file_read(
	bpb: &Bpb,
	cluster: u32,
	file_size: u32,
	offset: usize,
	buf: &mut [u8],
) -> usize {
	if offset >= file_size as usize || buf.is_empty() {
		return 0;
	}
	let readable = core::cmp::min(buf.len(), file_size as usize - offset);
	let chain = cluster_chain(bpb, cluster);
	let cluster_size = bpb.cluster_size_bytes();
	let mut done = 0;
	let mut file_pos = 0usize;

	let mut sector_buf = [0u8; SECTOR_SIZE];
	for &cl in &chain {
		let cl_end = file_pos + cluster_size;
		if cl_end <= offset {
			file_pos = cl_end;
			continue;
		}
		if file_pos >= offset + readable {
			break;
		}
		/* Read each sector in this cluster */
		let start_sector = bpb.cluster_to_sector(cl);
		for s in 0..bpb.sectors_per_cluster as u64 {
			let sec_start = file_pos;
			let sec_end = file_pos + SECTOR_SIZE;
			if sec_end <= offset || sec_start >= offset + readable {
				file_pos += SECTOR_SIZE;
				continue;
			}
			read_sector(start_sector + s, &mut sector_buf);

			let from = if sec_start < offset { offset - sec_start } else { 0 };
			let to = core::cmp::min(SECTOR_SIZE, offset + readable - sec_start);
			let buf_start = sec_start + from - offset;
			let copy_len = to - from;
			buf[buf_start..buf_start + copy_len]
				.copy_from_slice(&sector_buf[from..to]);
			done += copy_len;
			file_pos += SECTOR_SIZE;
		}
	}
	done
}

/*
 * file_write - Write bytes to a file at a byte offset
 *
 * Extends the cluster chain and updates the directory entry size
 * if writing beyond the current end of file.
 *
 * Return: Number of bytes written
 */
fn file_write(
	bpb: &Bpb,
	first_cluster: u32,
	old_size: u32,
	offset: usize,
	data: &[u8],
	/* Location of the 8.3 dir entry to update size */
	entry_sector: u64,
	entry_offset: usize,
) -> usize {
	if data.is_empty() {
		return 0;
	}

	let cluster_size = bpb.cluster_size_bytes();
	let needed_size = offset + data.len();
	let mut chain = cluster_chain(bpb, first_cluster);

	/* Extend chain if needed */
	let clusters_needed =
		(needed_size + cluster_size - 1) / cluster_size;
	while chain.len() < clusters_needed {
		let new_cl = match fat_alloc_cluster(bpb) {
			Some(c) => c,
			None => break,
		};
		if let Some(&last) = chain.last() {
			fat_write_entry(bpb, last, new_cl);
		}
		chain.push(new_cl);
	}

	let mut done = 0;
	let mut file_pos = 0usize;
	let mut sector_buf = [0u8; SECTOR_SIZE];

	for &cl in &chain {
		let start_sector = bpb.cluster_to_sector(cl);
		for s in 0..bpb.sectors_per_cluster as u64 {
			let sec_start = file_pos;
			let sec_end = file_pos + SECTOR_SIZE;
			if sec_end <= offset || sec_start >= offset + data.len() {
				file_pos += SECTOR_SIZE;
				continue;
			}
			/* Read-modify-write */
			read_sector(start_sector + s, &mut sector_buf);
			let from = if sec_start < offset { offset - sec_start } else { 0 };
			let to = core::cmp::min(
				SECTOR_SIZE,
				offset + data.len() - sec_start,
			);
			let data_start = sec_start + from - offset;
			let copy_len = to - from;
			sector_buf[from..to]
				.copy_from_slice(&data[data_start..data_start + copy_len]);
			write_sector(start_sector + s, &sector_buf);
			done += copy_len;
			file_pos += SECTOR_SIZE;
		}
	}

	/* Update file size in directory entry if grown */
	let new_size = core::cmp::max(old_size as usize, needed_size) as u32;
	if new_size != old_size {
		let mut entry_sec_buf = [0u8; SECTOR_SIZE];
		read_sector(entry_sector, &mut entry_sec_buf);
		let size_bytes = new_size.to_le_bytes();
		entry_sec_buf[entry_offset + 28] = size_bytes[0];
		entry_sec_buf[entry_offset + 29] = size_bytes[1];
		entry_sec_buf[entry_offset + 30] = size_bytes[2];
		entry_sec_buf[entry_offset + 31] = size_bytes[3];
		write_sector(entry_sector, &entry_sec_buf);
	}

	done
}

/* ------------------------------------------------------------------ */
/*  Directory Entry Creation                                            */
/* ------------------------------------------------------------------ */

/*
 * find_free_dir_slot - Find a free run of N directory entries in dir
 *
 * Returns (sector, offset_within_sector) of the first slot of the run.
 * Extends the cluster chain if necessary.
 */
fn find_free_dir_slots(
	bpb: &Bpb,
	dir_cluster: u32,
	needed: usize,
) -> Option<(u64, usize)> {
	let chain = cluster_chain(bpb, dir_cluster);
	let mut consecutive = 0;
	let mut first_sector = 0u64;
	let mut first_offset = 0usize;

	for &cl in &chain {
		let start_sector = bpb.cluster_to_sector(cl);
		for s in 0..bpb.sectors_per_cluster as u64 {
			let sector = start_sector + s;
			let mut buf = [0u8; SECTOR_SIZE];
			read_sector(sector, &mut buf);
			for entry_idx in 0..(SECTOR_SIZE / DIR_ENTRY_SIZE) {
				let off = entry_idx * DIR_ENTRY_SIZE;
				let first_byte = buf[off];
				if first_byte == 0x00 || first_byte == 0xE5 {
					if consecutive == 0 {
						first_sector = sector;
						first_offset = off;
					}
					consecutive += 1;
					if consecutive >= needed {
						return Some((first_sector, first_offset));
					}
				} else {
					consecutive = 0;
				}
			}
		}
	}

	/* Extend the directory cluster chain to get more space */
	let new_cl = fat_alloc_cluster(bpb)?;
	if let Some(&last) = chain.last() {
		fat_write_entry(bpb, last, new_cl);
	}
	/* Zero the new cluster */
	let start_sector = bpb.cluster_to_sector(new_cl);
	let zero = [0u8; SECTOR_SIZE];
	for s in 0..bpb.sectors_per_cluster as u64 {
		write_sector(start_sector + s, &zero);
	}
	Some((start_sector, 0))
}

/*
 * create_dir_entry - Create an LFN + 8.3 directory entry pair for name
 *
 * Returns the 8.3 entry's (sector, offset) on success, or None on failure.
 */
fn create_dir_entry(
	bpb: &Bpb,
	dir_cluster: u32,
	name: &str,
	attr: u8,
	first_cluster: u32,
) -> Option<(u64, usize)> {
	let sfn = build_sfn(name)?;
	let checksum = sfn_checksum(&sfn);

	/* Build LFN entries (in reverse order, so written first = last seq) */
	let name_utf16: Vec<u16> = name.encode_utf16().collect();
	let lfn_count = (name_utf16.len() + 12) / 13; /* ceil div */
	let total_entries = lfn_count + 1; /* LFN entries + 1 SFN entry */

	let (first_sector, first_off) =
		find_free_dir_slots(bpb, dir_cluster, total_entries)?;

	/* Pad name to multiple of 13 with 0xFFFF then 0x0000 */
	let mut padded = name_utf16.clone();
	if padded.len() % 13 != 0 {
		padded.push(0x0000);
		while padded.len() % 13 != 0 {
			padded.push(0xFFFF);
		}
	}

	/* We need to write entries sequentially: LFN (last seq first) then SFN */
	let mut sector = first_sector;
	let mut offset_in_sec = first_off;

	for lfn_idx in (0..lfn_count).rev() {
		let seq_num = lfn_idx as u8 + 1;
		let is_last = lfn_idx == lfn_count - 1;
		let flag = if is_last { seq_num | 0x40 } else { seq_num };

		let chunk = &padded[lfn_idx * 13..(lfn_idx + 1) * 13];
		let mut entry = [0u8; 32];
		entry[0] = flag;
		entry[11] = ATTR_LFN;
		entry[12] = 0;
		entry[13] = checksum;
		entry[26] = 0;
		entry[27] = 0;

		let positions = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
		for (i, &pos) in positions.iter().enumerate() {
			let w = chunk[i].to_le_bytes();
			entry[pos] = w[0];
			entry[pos + 1] = w[1];
		}

		write_dir_entry(bpb, sector, offset_in_sec, &entry);
		advance_dir_slot(bpb, &mut sector, &mut offset_in_sec);
	}

	/* Write the 8.3 entry */
	let sfn_sector = sector;
	let sfn_offset = offset_in_sec;
	let mut sfn_entry = [0u8; 32];
	sfn_entry[0..11].copy_from_slice(&sfn);
	sfn_entry[11] = attr;
	let (time_word, date_word) = fat32_timestamp();
	let t = time_word.to_le_bytes();
	let d = date_word.to_le_bytes();
	sfn_entry[14] = t[0]; sfn_entry[15] = t[1]; /* creation time */
	sfn_entry[16] = d[0]; sfn_entry[17] = d[1]; /* creation date */
	sfn_entry[22] = t[0]; sfn_entry[23] = t[1]; /* modified time */
	sfn_entry[24] = d[0]; sfn_entry[25] = d[1]; /* modified date */
	sfn_entry[20] = ((first_cluster >> 16) & 0xFF) as u8;
	sfn_entry[21] = ((first_cluster >> 24) & 0xFF) as u8;
	sfn_entry[26] = (first_cluster & 0xFF) as u8;
	sfn_entry[27] = ((first_cluster >> 8) & 0xFF) as u8;
	/* Size: 0 for new file */
	write_dir_entry(bpb, sfn_sector, sfn_offset, &sfn_entry);

	Some((sfn_sector, sfn_offset))
}

/* write_dir_entry - Write a 32-byte directory entry at (sector, offset) */
fn write_dir_entry(bpb: &Bpb, sector: u64, offset: usize, entry: &[u8; 32]) {
	let mut buf = [0u8; SECTOR_SIZE];
	read_sector(sector, &mut buf);
	buf[offset..offset + 32].copy_from_slice(entry);
	write_sector(sector, &buf);
}

/* advance_dir_slot - Advance (sector, offset) to the next directory slot */
fn advance_dir_slot(bpb: &Bpb, sector: &mut u64, offset: &mut usize) {
	*offset += DIR_ENTRY_SIZE;
	if *offset >= SECTOR_SIZE {
		*offset = 0;
		*sector += 1;
	}
}

/* ------------------------------------------------------------------ */
/*  Path Resolution                                                     */
/* ------------------------------------------------------------------ */

/*
 * find_entry_in_dir - Find a named entry in a directory cluster chain
 */
fn find_entry_in_dir(
	bpb: &Bpb,
	dir_cluster: u32,
	name: &str,
) -> Option<DirEntry> {
	let entries = read_dir_entries(bpb, dir_cluster);
	entries
		.into_iter()
		.find(|e| e.name.eq_ignore_ascii_case(name))
}

/* ------------------------------------------------------------------ */
/*  Global FAT32 State                                                  */
/* ------------------------------------------------------------------ */

struct Fat32State {
	bpb: Bpb,
}

static FAT32: Once<Mutex<Fat32State>> = Once::new();

/*
 * mount - Parse the BPB and initialize the global FAT32 state
 *
 * Must be called after setup_queues_global(). Returns true on success.
 */
pub fn mount() -> bool {
	if drivers::virtio::virtio_blk().is_none() {
		hal::serial_println!("FS: No VirtIO device");
		return false;
	}
	let mut sector0 = [0u8; SECTOR_SIZE];
	read_sector(0, &mut sector0);
	match Bpb::parse(&sector0) {
		Some(bpb) => {
			hal::serial_println!(
				"FS: FAT32 mounted (cluster_size={} bytes, root_cluster={})",
				bpb.cluster_size_bytes(),
				bpb.root_cluster,
			);
			FAT32.call_once(|| Mutex::new(Fat32State { bpb }));
			true
		}
		None => {
			hal::serial_println!("FS: FAT32 BPB parse failed — is disk formatted?");
			false
		}
	}
}

/* ------------------------------------------------------------------ */
/*  VFS INode Implementations                                           */
/* ------------------------------------------------------------------ */

/*
 * struct FatDirINode - Directory in the FAT32 filesystem
 * @cluster: First cluster of this directory (0 = root)
 */
pub struct FatDirINode {
	cluster: u32,
}

impl FatDirINode {
	/* root - Create the INode for the FAT32 root directory */
	pub fn root() -> Self {
		let cluster = FAT32
			.get()
			.map(|s| s.lock().bpb.root_cluster)
			.unwrap_or(2);
		Self { cluster }
	}

	pub fn new(cluster: u32) -> Self {
		Self { cluster }
	}
}

impl INode for FatDirINode {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
		0
	}

	fn write(&self, _offset: usize, _buf: &[u8]) -> usize {
		0
	}

	fn metadata(&self) -> FileType {
		FileType::Directory
	}

	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>> {
		let guard = FAT32.get()?.lock();
		let bpb = &guard.bpb;
		let entry = find_entry_in_dir(bpb, self.cluster, name)?;
		if entry.is_dir() {
			Some(Arc::new(FatDirINode::new(entry.first_cluster)))
		} else {
			Some(Arc::new(FatFileINode {
				first_cluster: entry.first_cluster,
				size: Mutex::new(entry.size),
				entry_sector: entry.entry_sector,
				entry_offset: entry.entry_offset,
			}))
		}
	}

	fn insert(
		&self,
		name: &str,
		_node: Arc<dyn INode>,
	) -> Result<(), &'static str> {
		let guard = FAT32.get().ok_or("fs not mounted")?;
		let bpb = &guard.lock().bpb;

		/* Reject duplicate names */
		if find_entry_in_dir(bpb, self.cluster, name).is_some() {
			return Err("file exists");
		}

		/* Allocate a cluster for the new file */
		let new_cluster =
			fat_alloc_cluster(bpb).ok_or("disk full")?;

		create_dir_entry(
			bpb,
			self.cluster,
			name,
			ATTR_ARCHIVE,
			new_cluster,
		)
		.ok_or("dir entry creation failed")?;
		Ok(())
	}

	fn mkdir(&self, name: &str) -> Result<(), &'static str> {
		let guard = FAT32.get().ok_or("fs not mounted")?;
		let bpb = &guard.lock().bpb;

		/* Reject duplicate names */
		if find_entry_in_dir(bpb, self.cluster, name).is_some() {
			return Err("file exists");
		}

		/* Allocate cluster for the new directory */
		let new_cluster = fat_alloc_cluster(bpb).ok_or("disk full")?;

		/* Zero out the new cluster */
		let start_sector = bpb.cluster_to_sector(new_cluster);
		let zero = [0u8; SECTOR_SIZE];
		for s in 0..bpb.sectors_per_cluster as u64 {
			write_sector(start_sector + s, &zero);
		}

		let (time_word, date_word) = fat32_timestamp();
		let t = time_word.to_le_bytes();
		let d = date_word.to_le_bytes();

		/* Write '.' entry at offset 0 */
		let mut dot_entry = [0u8; 32];
		dot_entry[0..8].copy_from_slice(b".       ");
		dot_entry[8..11].copy_from_slice(b"   ");
		dot_entry[11] = ATTR_DIRECTORY;
		dot_entry[14] = t[0]; dot_entry[15] = t[1];
		dot_entry[16] = d[0]; dot_entry[17] = d[1];
		dot_entry[20] = ((new_cluster >> 16) & 0xFF) as u8;
		dot_entry[21] = ((new_cluster >> 24) & 0xFF) as u8;
		dot_entry[26] = (new_cluster & 0xFF) as u8;
		dot_entry[27] = ((new_cluster >> 8) & 0xFF) as u8;
		write_dir_entry(bpb, start_sector, 0, &dot_entry);

		/* Write '..' entry at offset 32 */
		let mut dotdot_entry = [0u8; 32];
		dotdot_entry[0..8].copy_from_slice(b"..      ");
		dotdot_entry[8..11].copy_from_slice(b"   ");
		dotdot_entry[11] = ATTR_DIRECTORY;
		dotdot_entry[14] = t[0]; dotdot_entry[15] = t[1];
		dotdot_entry[16] = d[0]; dotdot_entry[17] = d[1];
		dotdot_entry[20] = ((self.cluster >> 16) & 0xFF) as u8;
		dotdot_entry[21] = ((self.cluster >> 24) & 0xFF) as u8;
		dotdot_entry[26] = (self.cluster & 0xFF) as u8;
		dotdot_entry[27] = ((self.cluster >> 8) & 0xFF) as u8;
		write_dir_entry(bpb, start_sector, 32, &dotdot_entry);

		/* Create the directory entry in the parent */
		create_dir_entry(bpb, self.cluster, name, ATTR_DIRECTORY, new_cluster)
			.ok_or("dir entry creation failed")?;

		Ok(())
	}

	fn unlink(&self, name: &str) -> Result<(), &'static str> {
		let guard = FAT32.get().ok_or("fs not mounted")?;
		let bpb = &guard.lock().bpb;

		let entry = find_entry_in_dir(bpb, self.cluster, name)
			.ok_or("not found")?;

		/* Free every cluster in the file's chain */
		for cl in cluster_chain(bpb, entry.first_cluster) {
			fat_write_entry(bpb, cl, FAT32_FREE);
		}

		/* Mark LFN entries and 8.3 entry as deleted (0xE5) */
		let mut sector = entry.lfn_start_sector;
		let mut offset = entry.lfn_start_offset;
		loop {
			let mut buf = [0u8; SECTOR_SIZE];
			read_sector(sector, &mut buf);
			buf[offset] = 0xE5;
			write_sector(sector, &buf);
			if sector == entry.entry_sector && offset == entry.entry_offset {
				break;
			}
			advance_dir_slot(bpb, &mut sector, &mut offset);
		}

		Ok(())
	}
}

/*
 * struct FatFileINode - File in the FAT32 filesystem
 */
pub struct FatFileINode {
	first_cluster: u32,
	size: Mutex<u32>,        /* Current file size (updated on write) */
	entry_sector: u64,       /* Sector containing the 8.3 dir entry  */
	entry_offset: usize,     /* Offset within that sector             */
}

impl INode for FatFileINode {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		let guard = match FAT32.get() {
			Some(g) => g.lock(),
			None => return 0,
		};
		let bpb = &guard.bpb;
		let size = *self.size.lock();
		file_read(bpb, self.first_cluster, size, offset, buf)
	}

	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		let guard = match FAT32.get() {
			Some(g) => g.lock(),
			None => return 0,
		};
		let bpb = &guard.bpb;
		let mut size = self.size.lock();
		let written = file_write(
			bpb,
			self.first_cluster,
			*size,
			offset,
			buf,
			self.entry_sector,
			self.entry_offset,
		);
		let new_size = core::cmp::max(*size as usize, offset + written);
		*size = new_size as u32;
		written
	}

	fn metadata(&self) -> FileType {
		FileType::File
	}

	fn size(&self) -> usize {
		*self.size.lock() as usize
	}
}
