/*
 * fat32/mod.rs - FAT32 Filesystem Driver
 *
 * Implements FsDriver for FAT32. All existing FAT32 logic is preserved
 * here unchanged; only the public API is extended with init() and the
 * FsDriver impl so the registry can probe and mount FAT32 volumes.
 *
 * Register at boot: fs::fat32::init()
 */
extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, Once};
use vfs::{FileType, INode};
use crate::{BlockDev, FsDriver};

/* ------------------------------------------------------------------ */
/*  FAT32 Constants                                                     */
/* ------------------------------------------------------------------ */

const SECTOR_SIZE: usize = 512;
const FAT32_EOC: u32 = 0x0FFF_FFF8;
const FAT32_FREE: u32 = 0;
const FAT32_MASK: u32 = 0x0FFF_FFFF;
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

fn fat32_timestamp() -> (u16, u16) {
	let ticks = apic::timer::ticks();
	let secs = ticks / 625;
	let hours = (secs / 3600) % 24;
	let minutes = (secs / 60) % 60;
	let two_secs = (secs % 60) / 2;
	let time_word = ((hours << 11) | (minutes << 5) | two_secs) as u16;
	let date_word: u16 = (0 << 9) | (1 << 5) | 1;
	(time_word, date_word)
}

/* ------------------------------------------------------------------ */
/*  BPB                                                                 */
/* ------------------------------------------------------------------ */

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
	fn parse(buf: &[u8; SECTOR_SIZE]) -> Option<Self> {
		if buf[510] != 0x55 || buf[511] != 0xAA { return None; }
		let bytes_per_sector = u16::from_le_bytes([buf[11], buf[12]]);
		let sectors_per_cluster = buf[13];
		let reserved_sectors = u16::from_le_bytes([buf[14], buf[15]]);
		let fat_count = buf[16];
		let total_sectors_16 = u16::from_le_bytes([buf[19], buf[20]]);
		let sectors_per_fat_16 = u16::from_le_bytes([buf[22], buf[23]]);
		let total_sectors_32 = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
		let sectors_per_fat_32 = u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]);
		let root_cluster = u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]);
		let sectors_per_fat = if sectors_per_fat_16 == 0 { sectors_per_fat_32 } else { sectors_per_fat_16 as u32 };
		let total_sectors = if total_sectors_16 == 0 { total_sectors_32 } else { total_sectors_16 as u32 };
		if bytes_per_sector == 0 || sectors_per_cluster == 0 || fat_count == 0 || sectors_per_fat == 0 { return None; }
		/* Check FAT32 signature */
		if &buf[82..90] != b"FAT32   " { return None; }
		Some(Bpb { bytes_per_sector, sectors_per_cluster, reserved_sectors, fat_count, total_sectors, sectors_per_fat, root_cluster })
	}
	fn fat_start_sector(&self) -> u64 { self.reserved_sectors as u64 }
	fn data_start_sector(&self) -> u64 { self.reserved_sectors as u64 + self.fat_count as u64 * self.sectors_per_fat as u64 }
	fn cluster_to_sector(&self, cluster: u32) -> u64 { self.data_start_sector() + (cluster as u64 - 2) * self.sectors_per_cluster as u64 }
	fn cluster_size_bytes(&self) -> usize { self.sectors_per_cluster as usize * SECTOR_SIZE }
	fn fat_sector_for(&self, cluster: u32) -> u64 { self.fat_start_sector() + (cluster as u64 * 4) / SECTOR_SIZE as u64 }
	fn fat_offset_in_sector(&self, cluster: u32) -> usize { (cluster as usize * 4) % SECTOR_SIZE }
}

/* ------------------------------------------------------------------ */
/*  Raw I/O (uses global VirtIO device)                                 */
/* ------------------------------------------------------------------ */

fn read_sector(sector: u64, buf: &mut [u8; SECTOR_SIZE]) {
	if let Some(blk) = drivers::virtio::virtio_blk() {
		let _ = blk.lock().read_sector(sector, buf);
	}
}

fn write_sector(sector: u64, buf: &[u8; SECTOR_SIZE]) {
	if let Some(blk) = drivers::virtio::virtio_blk() {
		let _ = blk.lock().write_sector(sector, buf);
	}
}

/* ------------------------------------------------------------------ */
/*  FAT Cluster Chain                                                   */
/* ------------------------------------------------------------------ */

fn fat_read_entry(bpb: &Bpb, cluster: u32) -> u32 {
	let sector = bpb.fat_sector_for(cluster);
	let offset = bpb.fat_offset_in_sector(cluster);
	let mut buf = [0u8; SECTOR_SIZE];
	read_sector(sector, &mut buf);
	u32::from_le_bytes([buf[offset], buf[offset+1], buf[offset+2], buf[offset+3]]) & FAT32_MASK
}

fn fat_write_entry(bpb: &Bpb, cluster: u32, value: u32) {
	for fat_num in 0..bpb.fat_count as u64 {
		let fat_base = bpb.reserved_sectors as u64 + fat_num * bpb.sectors_per_fat as u64;
		let sector = fat_base + (cluster as u64 * 4) / SECTOR_SIZE as u64;
		let offset = (cluster as usize * 4) % SECTOR_SIZE;
		let mut buf = [0u8; SECTOR_SIZE];
		read_sector(sector, &mut buf);
		let bytes = (value & FAT32_MASK).to_le_bytes();
		buf[offset] = bytes[0]; buf[offset+1] = bytes[1]; buf[offset+2] = bytes[2];
		buf[offset+3] = (buf[offset+3] & 0xF0) | bytes[3];
		write_sector(sector, &buf);
	}
}

fn fat_alloc_cluster(bpb: &Bpb) -> Option<u32> {
	let max_cluster = (bpb.total_sectors - bpb.data_start_sector() as u32) / bpb.sectors_per_cluster as u32 + 2;
	for cluster in 2..max_cluster {
		if fat_read_entry(bpb, cluster) == FAT32_FREE {
			fat_write_entry(bpb, cluster, FAT32_EOC);
			return Some(cluster);
		}
	}
	None
}

fn cluster_chain(bpb: &Bpb, first_cluster: u32) -> Vec<u32> {
	let mut chain = Vec::new();
	let mut cur = first_cluster;
	while cur >= 2 && cur < FAT32_EOC {
		chain.push(cur);
		let next = fat_read_entry(bpb, cur);
		if next >= FAT32_EOC || next < 2 { break; }
		cur = next;
	}
	chain
}

/* ------------------------------------------------------------------ */
/*  Directory Entry Parsing                                             */
/* ------------------------------------------------------------------ */

#[derive(Debug, Clone)]
struct DirEntry {
	name: String,
	attr: u8,
	first_cluster: u32,
	size: u32,
	entry_sector: u64,
	entry_offset: usize,
	lfn_start_sector: u64,
	lfn_start_offset: usize,
}

impl DirEntry {
	fn is_dir(&self) -> bool { self.attr & ATTR_DIRECTORY != 0 }
}

fn read_lfn_chars(raw: &[u8; 32]) -> [u16; 13] {
	let mut chars = [0u16; 13];
	let offsets: [usize; 13] = [1,3,5,7,9,14,16,18,20,22,24,28,30];
	for (i, &off) in offsets.iter().enumerate() {
		chars[i] = u16::from_le_bytes([raw[off], raw[off+1]]);
	}
	chars
}

fn parse_sfn(raw: &[u8; 11]) -> String {
	let name = core::str::from_utf8(&raw[0..8]).unwrap_or("").trim_end_matches(' ');
	let ext = core::str::from_utf8(&raw[8..11]).unwrap_or("").trim_end_matches(' ');
	if ext.is_empty() { name.to_string() } else { alloc::format!("{}.{}", name, ext) }
}

fn build_sfn(name: &str) -> Option<[u8; 11]> {
	let mut sfn = [b' '; 11];
	let (base, ext) = if let Some(dot) = name.rfind('.') { (&name[..dot], &name[dot+1..]) } else { (name, "") };
	if base.is_empty() || base.len() > 8 || ext.len() > 3 { return None; }
	let invalid = |c: char| matches!(c, ' '|'.'|'"'|'*'|'+'|','|'/'|':'|';'|'<'|'='|'>'|'?'|'['|'\\'|']'|'|');
	if base.chars().any(invalid) || ext.chars().any(invalid) { return None; }
	for (i, b) in base.bytes().enumerate() { sfn[i] = b.to_ascii_uppercase(); }
	for (i, b) in ext.bytes().enumerate() { sfn[8+i] = b.to_ascii_uppercase(); }
	Some(sfn)
}

fn sfn_checksum(sfn: &[u8; 11]) -> u8 {
	let mut sum: u8 = 0;
	for &b in sfn.iter() { sum = (sum >> 1).wrapping_add(sum << 7).wrapping_add(b); }
	sum
}

fn read_dir_entries(bpb: &Bpb, first_cluster: u32) -> Vec<DirEntry> {
	let mut entries = Vec::new();
	let chain = cluster_chain(bpb, first_cluster);
	let mut lfn_buf: Vec<u16> = Vec::new();
	let mut lfn_start: Option<(u64, usize)> = None;

	for cluster in chain {
		let start_sector = bpb.cluster_to_sector(cluster);
		for s in 0..bpb.sectors_per_cluster as u64 {
			let mut sector_buf = [0u8; SECTOR_SIZE];
			let sector = start_sector + s;
			read_sector(sector, &mut sector_buf);
			for entry_idx in 0..(SECTOR_SIZE / DIR_ENTRY_SIZE) {
				let offset = entry_idx * DIR_ENTRY_SIZE;
				let raw: &[u8; 32] = sector_buf[offset..offset+32].try_into().unwrap();
				if raw[0] == 0x00 { return entries; }
				if raw[0] == 0xE5 { lfn_buf.clear(); lfn_start = None; continue; }
				if raw[11] == ATTR_LFN {
					let is_last = raw[0] & 0x40 != 0;
					if is_last { lfn_buf.clear(); lfn_start = Some((sector, offset)); }
					let chars = read_lfn_chars(raw);
					let chunk: Vec<u16> = chars.iter().take_while(|&&c| c != 0).copied().collect();
					let mut combined = chunk;
					combined.extend_from_slice(&lfn_buf);
					lfn_buf = combined;
					continue;
				}
				if raw[11] & ATTR_VOLUME_ID != 0 && raw[11] & ATTR_DIRECTORY == 0 { lfn_buf.clear(); lfn_start = None; continue; }
				let sfn_bytes: [u8; 11] = raw[0..11].try_into().unwrap();
				let sfn_name = parse_sfn(&sfn_bytes);
				let name = if !lfn_buf.is_empty() {
					let s: String = lfn_buf.iter().take_while(|&&c| c != 0).filter_map(|&c| char::from_u32(c as u32)).collect();
					lfn_buf.clear();
					s
				} else { sfn_name };
				let fc_hi = u16::from_le_bytes([raw[20], raw[21]]) as u32;
				let fc_lo = u16::from_le_bytes([raw[26], raw[27]]) as u32;
				let first_cluster_val = (fc_hi << 16) | fc_lo;
				let size = u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]);
				let attr = raw[11];
				if name == "." || name == ".." { lfn_start = None; continue; }
				let (ls, lo) = lfn_start.unwrap_or((sector, offset));
				entries.push(DirEntry { name, attr, first_cluster: first_cluster_val, size, entry_sector: sector, entry_offset: offset, lfn_start_sector: ls, lfn_start_offset: lo });
				lfn_start = None;
			}
		}
	}
	entries
}

/* ------------------------------------------------------------------ */
/*  File Read / Write                                                   */
/* ------------------------------------------------------------------ */

fn file_read(bpb: &Bpb, cluster: u32, file_size: u32, offset: usize, buf: &mut [u8]) -> usize {
	if offset >= file_size as usize || buf.is_empty() { return 0; }
	let readable = core::cmp::min(buf.len(), file_size as usize - offset);
	let chain = cluster_chain(bpb, cluster);
	let cluster_size = bpb.cluster_size_bytes();
	let mut done = 0;
	let mut file_pos = 0usize;
	let mut sector_buf = [0u8; SECTOR_SIZE];
	for &cl in &chain {
		let cl_end = file_pos + cluster_size;
		if cl_end <= offset { file_pos = cl_end; continue; }
		if file_pos >= offset + readable { break; }
		let start_sector = bpb.cluster_to_sector(cl);
		for s in 0..bpb.sectors_per_cluster as u64 {
			let sec_start = file_pos;
			let sec_end = file_pos + SECTOR_SIZE;
			if sec_end <= offset || sec_start >= offset + readable { file_pos += SECTOR_SIZE; continue; }
			read_sector(start_sector + s, &mut sector_buf);
			let from = if sec_start < offset { offset - sec_start } else { 0 };
			let to = core::cmp::min(SECTOR_SIZE, offset + readable - sec_start);
			let buf_start = sec_start + from - offset;
			let copy_len = to - from;
			buf[buf_start..buf_start+copy_len].copy_from_slice(&sector_buf[from..to]);
			done += copy_len;
			file_pos += SECTOR_SIZE;
		}
	}
	done
}

fn file_write(bpb: &Bpb, first_cluster: u32, old_size: u32, offset: usize, data: &[u8], entry_sector: u64, entry_offset: usize) -> usize {
	if data.is_empty() { return 0; }
	let cluster_size = bpb.cluster_size_bytes();
	let needed_size = offset + data.len();
	let mut chain = cluster_chain(bpb, first_cluster);
	let clusters_needed = (needed_size + cluster_size - 1) / cluster_size;
	while chain.len() < clusters_needed {
		let new_cl = match fat_alloc_cluster(bpb) { Some(c) => c, None => break };
		if let Some(&last) = chain.last() { fat_write_entry(bpb, last, new_cl); }
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
			if sec_end <= offset || sec_start >= offset + data.len() { file_pos += SECTOR_SIZE; continue; }
			read_sector(start_sector + s, &mut sector_buf);
			let from = if sec_start < offset { offset - sec_start } else { 0 };
			let to = core::cmp::min(SECTOR_SIZE, offset + data.len() - sec_start);
			let data_start = sec_start + from - offset;
			let copy_len = to - from;
			sector_buf[from..to].copy_from_slice(&data[data_start..data_start+copy_len]);
			write_sector(start_sector + s, &sector_buf);
			done += copy_len;
			file_pos += SECTOR_SIZE;
		}
	}
	let new_size = core::cmp::max(old_size as usize, needed_size) as u32;
	if new_size != old_size {
		let mut entry_sec_buf = [0u8; SECTOR_SIZE];
		read_sector(entry_sector, &mut entry_sec_buf);
		let size_bytes = new_size.to_le_bytes();
		entry_sec_buf[entry_offset+28] = size_bytes[0];
		entry_sec_buf[entry_offset+29] = size_bytes[1];
		entry_sec_buf[entry_offset+30] = size_bytes[2];
		entry_sec_buf[entry_offset+31] = size_bytes[3];
		write_sector(entry_sector, &entry_sec_buf);
	}
	done
}

/* ------------------------------------------------------------------ */
/*  Directory Entry Creation                                            */
/* ------------------------------------------------------------------ */

fn find_free_dir_slots(bpb: &Bpb, dir_cluster: u32, needed: usize) -> Option<(u64, usize)> {
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
					if consecutive == 0 { first_sector = sector; first_offset = off; }
					consecutive += 1;
					if consecutive >= needed { return Some((first_sector, first_offset)); }
				} else { consecutive = 0; }
			}
		}
	}
	let new_cl = fat_alloc_cluster(bpb)?;
	if let Some(&last) = chain.last() { fat_write_entry(bpb, last, new_cl); }
	let start_sector = bpb.cluster_to_sector(new_cl);
	let zero = [0u8; SECTOR_SIZE];
	for s in 0..bpb.sectors_per_cluster as u64 { write_sector(start_sector + s, &zero); }
	Some((start_sector, 0))
}

fn create_dir_entry(bpb: &Bpb, dir_cluster: u32, name: &str, attr: u8, first_cluster: u32) -> Option<(u64, usize)> {
	let sfn = build_sfn(name)?;
	let checksum = sfn_checksum(&sfn);
	let name_utf16: Vec<u16> = name.encode_utf16().collect();
	let lfn_count = (name_utf16.len() + 12) / 13;
	let total_entries = lfn_count + 1;
	let (first_sector, first_off) = find_free_dir_slots(bpb, dir_cluster, total_entries)?;
	let mut padded = name_utf16.clone();
	if padded.len() % 13 != 0 {
		padded.push(0x0000);
		while padded.len() % 13 != 0 { padded.push(0xFFFF); }
	}
	let mut sector = first_sector;
	let mut offset_in_sec = first_off;
	for lfn_idx in (0..lfn_count).rev() {
		let seq_num = lfn_idx as u8 + 1;
		let is_last = lfn_idx == lfn_count - 1;
		let flag = if is_last { seq_num | 0x40 } else { seq_num };
		let chunk = &padded[lfn_idx * 13..(lfn_idx + 1) * 13];
		let mut entry = [0u8; 32];
		entry[0] = flag; entry[11] = ATTR_LFN; entry[12] = 0; entry[13] = checksum; entry[26] = 0; entry[27] = 0;
		let positions = [1usize,3,5,7,9,14,16,18,20,22,24,28,30];
		for (i, &pos) in positions.iter().enumerate() {
			let w = chunk[i].to_le_bytes();
			entry[pos] = w[0]; entry[pos+1] = w[1];
		}
		write_dir_entry(bpb, sector, offset_in_sec, &entry);
		advance_dir_slot(bpb, &mut sector, &mut offset_in_sec);
	}
	let sfn_sector = sector;
	let sfn_offset = offset_in_sec;
	let mut sfn_entry = [0u8; 32];
	sfn_entry[0..11].copy_from_slice(&sfn);
	sfn_entry[11] = attr;
	let (time_word, date_word) = fat32_timestamp();
	let t = time_word.to_le_bytes(); let d = date_word.to_le_bytes();
	sfn_entry[14] = t[0]; sfn_entry[15] = t[1]; sfn_entry[16] = d[0]; sfn_entry[17] = d[1];
	sfn_entry[22] = t[0]; sfn_entry[23] = t[1]; sfn_entry[24] = d[0]; sfn_entry[25] = d[1];
	sfn_entry[20] = ((first_cluster >> 16) & 0xFF) as u8;
	sfn_entry[21] = ((first_cluster >> 24) & 0xFF) as u8;
	sfn_entry[26] = (first_cluster & 0xFF) as u8;
	sfn_entry[27] = ((first_cluster >> 8) & 0xFF) as u8;
	write_dir_entry(bpb, sfn_sector, sfn_offset, &sfn_entry);
	Some((sfn_sector, sfn_offset))
}

fn write_dir_entry(_bpb: &Bpb, sector: u64, offset: usize, entry: &[u8; 32]) {
	let mut buf = [0u8; SECTOR_SIZE];
	read_sector(sector, &mut buf);
	buf[offset..offset+32].copy_from_slice(entry);
	write_sector(sector, &buf);
}

fn advance_dir_slot(_bpb: &Bpb, sector: &mut u64, offset: &mut usize) {
	*offset += DIR_ENTRY_SIZE;
	if *offset >= SECTOR_SIZE { *offset = 0; *sector += 1; }
}

fn find_entry_in_dir(bpb: &Bpb, dir_cluster: u32, name: &str) -> Option<DirEntry> {
	read_dir_entries(bpb, dir_cluster).into_iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/* ------------------------------------------------------------------ */
/*  Global FAT32 State                                                  */
/* ------------------------------------------------------------------ */

struct Fat32State { bpb: Bpb }
static FAT32: Once<Mutex<Fat32State>> = Once::new();

/* ------------------------------------------------------------------ */
/*  VFS INode Implementations                                           */
/* ------------------------------------------------------------------ */

pub struct FatDirINode { cluster: u32 }

impl FatDirINode {
	pub fn root() -> Self {
		let cluster = FAT32.get().map(|s| s.lock().bpb.root_cluster).unwrap_or(2);
		Self { cluster }
	}
	pub fn new(cluster: u32) -> Self { Self { cluster } }
}

impl INode for FatDirINode {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }
	fn write(&self, _offset: usize, _buf: &[u8]) -> usize { 0 }
	fn metadata(&self) -> FileType { FileType::Directory }

	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>> {
		let guard = FAT32.get()?.lock();
		let bpb = &guard.bpb;
		let entry = find_entry_in_dir(bpb, self.cluster, name)?;
		if entry.is_dir() {
			Some(Arc::new(FatDirINode::new(entry.first_cluster)))
		} else {
			Some(Arc::new(FatFileINode { first_cluster: entry.first_cluster, size: Mutex::new(entry.size), entry_sector: entry.entry_sector, entry_offset: entry.entry_offset }))
		}
	}

	fn insert(&self, name: &str, _node: Arc<dyn INode>) -> Result<(), &'static str> {
		let guard = FAT32.get().ok_or("fs not mounted")?;
		let bpb = &guard.lock().bpb;
		if find_entry_in_dir(bpb, self.cluster, name).is_some() { return Err("file exists"); }
		let new_cluster = fat_alloc_cluster(bpb).ok_or("disk full")?;
		create_dir_entry(bpb, self.cluster, name, ATTR_ARCHIVE, new_cluster).ok_or("dir entry creation failed")?;
		Ok(())
	}

	fn create_file(&self, name: &str) -> Result<Arc<dyn INode>, &'static str> {
		let guard = FAT32.get().ok_or("fs not mounted")?;
		let bpb = &guard.lock().bpb;
		if find_entry_in_dir(bpb, self.cluster, name).is_some() { return Err("file exists"); }
		let new_cluster = fat_alloc_cluster(bpb).ok_or("disk full")?;
		let (es, eo) = create_dir_entry(bpb, self.cluster, name, ATTR_ARCHIVE, new_cluster).ok_or("dir entry creation failed")?;
		Ok(Arc::new(FatFileINode { first_cluster: new_cluster, size: Mutex::new(0), entry_sector: es, entry_offset: eo }))
	}

	fn mkdir(&self, name: &str) -> Result<(), &'static str> {
		let guard = FAT32.get().ok_or("fs not mounted")?;
		let bpb = &guard.lock().bpb;
		if find_entry_in_dir(bpb, self.cluster, name).is_some() { return Err("file exists"); }
		let new_cluster = fat_alloc_cluster(bpb).ok_or("disk full")?;
		let start_sector = bpb.cluster_to_sector(new_cluster);
		let zero = [0u8; SECTOR_SIZE];
		for s in 0..bpb.sectors_per_cluster as u64 { write_sector(start_sector + s, &zero); }
		let (time_word, date_word) = fat32_timestamp();
		let t = time_word.to_le_bytes(); let d = date_word.to_le_bytes();
		let mut dot = [0u8; 32];
		dot[0..8].copy_from_slice(b".       "); dot[8..11].copy_from_slice(b"   "); dot[11] = ATTR_DIRECTORY;
		dot[14] = t[0]; dot[15] = t[1]; dot[16] = d[0]; dot[17] = d[1];
		dot[20] = ((new_cluster >> 16) & 0xFF) as u8; dot[21] = ((new_cluster >> 24) & 0xFF) as u8;
		dot[26] = (new_cluster & 0xFF) as u8; dot[27] = ((new_cluster >> 8) & 0xFF) as u8;
		write_dir_entry(bpb, start_sector, 0, &dot);
		let mut dotdot = [0u8; 32];
		dotdot[0..8].copy_from_slice(b"..      "); dotdot[8..11].copy_from_slice(b"   "); dotdot[11] = ATTR_DIRECTORY;
		dotdot[14] = t[0]; dotdot[15] = t[1]; dotdot[16] = d[0]; dotdot[17] = d[1];
		dotdot[20] = ((self.cluster >> 16) & 0xFF) as u8; dotdot[21] = ((self.cluster >> 24) & 0xFF) as u8;
		dotdot[26] = (self.cluster & 0xFF) as u8; dotdot[27] = ((self.cluster >> 8) & 0xFF) as u8;
		write_dir_entry(bpb, start_sector, 32, &dotdot);
		create_dir_entry(bpb, self.cluster, name, ATTR_DIRECTORY, new_cluster).ok_or("dir entry creation failed")?;
		Ok(())
	}

	fn unlink(&self, name: &str) -> Result<(), &'static str> {
		let guard = FAT32.get().ok_or("fs not mounted")?;
		let bpb = &guard.lock().bpb;
		let entry = find_entry_in_dir(bpb, self.cluster, name).ok_or("not found")?;
		for cl in cluster_chain(bpb, entry.first_cluster) { fat_write_entry(bpb, cl, FAT32_FREE); }
		let mut sector = entry.lfn_start_sector;
		let mut offset = entry.lfn_start_offset;
		loop {
			let mut buf = [0u8; SECTOR_SIZE];
			read_sector(sector, &mut buf);
			buf[offset] = 0xE5;
			write_sector(sector, &buf);
			if sector == entry.entry_sector && offset == entry.entry_offset { break; }
			advance_dir_slot(bpb, &mut sector, &mut offset);
		}
		Ok(())
	}

	fn readdir(&self) -> Option<Vec<(String, FileType)>> {
		let guard = FAT32.get()?.lock();
		let entries = read_dir_entries(&guard.bpb, self.cluster);
		Some(entries.into_iter().map(|e| {
			let ft = if e.is_dir() { FileType::Directory } else { FileType::File };
			(e.name, ft)
		}).collect())
	}
}

pub struct FatFileINode {
	first_cluster: u32,
	size: Mutex<u32>,
	entry_sector: u64,
	entry_offset: usize,
}

impl INode for FatFileINode {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		let guard = match FAT32.get() { Some(g) => g.lock(), None => return 0 };
		file_read(&guard.bpb, self.first_cluster, *self.size.lock(), offset, buf)
	}
	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		let guard = match FAT32.get() { Some(g) => g.lock(), None => return 0 };
		let mut size = self.size.lock();
		let written = file_write(&guard.bpb, self.first_cluster, *size, offset, buf, self.entry_sector, self.entry_offset);
		*size = core::cmp::max(*size as usize, offset + written) as u32;
		written
	}
	fn metadata(&self) -> FileType { FileType::File }
	fn size(&self) -> usize { *self.size.lock() as usize }
}

/* ------------------------------------------------------------------ */
/*  FsDriver impl + registration                                        */
/* ------------------------------------------------------------------ */

struct Fat32Driver;

impl FsDriver for Fat32Driver {
	fn name(&self) -> &'static str { "fat32" }

	fn probe(&self, dev: &dyn BlockDev) -> bool {
		let mut buf = [0u8; 512];
		dev.read_block(0, &mut buf);
		buf[510] == 0x55 && buf[511] == 0xAA && &buf[82..90] == b"FAT32   "
	}

	fn mount(&self, _dev: Arc<dyn BlockDev>) -> Option<Arc<dyn INode>> {
		/* FAT32 driver still uses the global VirtIO device internally */
		let mut sector0 = [0u8; 512];
		read_sector(0, &mut sector0);
		let bpb = Bpb::parse(&sector0)?;
		FAT32.call_once(|| Mutex::new(Fat32State { bpb }));
		Some(Arc::new(FatDirINode::root()))
	}
}

/*
 * init - Register the FAT32 driver with the global fs registry
 *
 * Call once at boot before mounting.
 */
pub fn init() {
	crate::register(Arc::new(Fat32Driver));
}
