/*
 * ext4/dir.rs - Directory Entry Parser
 *
 * ext4 linear directory entries are byte-for-byte identical to ext2.
 * The only difference is block resolution: use extent::get_block() instead
 * of the ext2 direct/indirect map.
 */

extern crate alloc;
use super::bgdt::BgDescTable;
use super::extent;
use super::inode::Inode;
use super::superblock::Superblock;
use crate::BlockDev;
use alloc::string::String;
use alloc::vec::Vec;

pub const EXT4_FT_REG_FILE: u8 = 1;
pub const EXT4_FT_DIR: u8 = 2;

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

fn for_each_entry<F>(block: &[u8], mut f: F)
where
	F: FnMut(usize, u32, usize, &[u8]) -> bool,
{
	let mut off = 0usize;
	while off + 8 <= block.len() {
		let ino = u32::from_le_bytes([block[off], block[off + 1], block[off + 2], block[off + 3]]);
		let rec_len = u16::from_le_bytes([block[off + 4], block[off + 5]]) as usize;
		let nlen = block[off + 6] as usize;
		if rec_len < 8 || off + rec_len > block.len() {
			break;
		}
		let name = &block[off + 8..off + 8 + nlen.min(block.len() - off - 8)];
		if !f(off, ino, rec_len, name) {
			return;
		}
		off += rec_len;
	}
}

pub fn lookup_in_dir(
	dev: &dyn BlockDev,
	sb: &Superblock,
	bgdt: &BgDescTable,
	dir_ino: &Inode,
	name: &str,
) -> Option<u32> {
	/* Try HTree first if directory is indexed */
	if super::htree::is_htree_indexed(dev, sb, dir_ino) {
		if let Some(ino) = super::htree::lookup_htree(dev, sb, dir_ino, name) {
			return Some(ino);
		}
	}

	/* Fall back to linear scan */
	let bsz = sb.block_size();
	let n_blks = (dir_ino.size() + bsz - 1) / bsz;
	for bi in 0..n_blks as u32 {
		let phys = extent::get_block(dev, sb, &dir_ino.block, bi)?;
		let block = read_block(dev, sb, phys);
		let mut found = None;
		for_each_entry(&block, |_, ino, _, nb| {
			if ino != 0 && nb == name.as_bytes() {
				found = Some(ino);
				false
			} else {
				true
			}
		});
		if found.is_some() {
			return found;
		}
	}
	None
}

pub fn readdir(
	dev: &dyn BlockDev,
	sb: &Superblock,
	bgdt: &BgDescTable,
	dir_ino: &Inode,
) -> Vec<(String, vfs::FileType)> {
	let bsz = sb.block_size();
	let n_blks = (dir_ino.size() + bsz - 1) / bsz;
	let mut out = Vec::new();
	for bi in 0..n_blks as u32 {
		let phys = match extent::get_block(dev, sb, &dir_ino.block, bi) {
			Some(p) => p,
			None => continue,
		};
		let block = read_block(dev, sb, phys);
		for_each_entry(&block, |_, ino, _, nb| {
			if ino != 0 {
				if let Ok(s) = core::str::from_utf8(nb) {
					let name = String::from(s);
					if name != "." && name != ".." {
						let ftype = Inode::read(dev, sb, bgdt, ino)
							.map(|i| {
								if i.is_dir() {
									vfs::FileType::Directory
								} else {
									vfs::FileType::File
								}
							})
							.unwrap_or(vfs::FileType::File);
						out.push((name, ftype));
					}
				}
			}
			true
		});
	}
	out
}

pub fn add_entry(
	dev: &dyn BlockDev,
	sb: &mut Superblock,
	bgdt: &mut BgDescTable,
	dir_ino: &mut Inode,
	name: &str,
	child_ino: u32,
	file_type: u8,
) {
	let bsz = sb.block_size();
	let name_b = name.as_bytes();
	let nlen = name_b.len();
	let needed = ((8 + nlen + 3) & !3) as u16;
	let n_blks = (dir_ino.size() + bsz - 1) / bsz;

	for bi in 0..n_blks as u32 {
		let phys = match extent::get_block(dev, sb, &dir_ino.block, bi) {
			Some(p) => p,
			None => continue,
		};
		let mut block = read_block(dev, sb, phys);
		let mut off = 0usize;
		let mut inserted = false;
		loop {
			if off + 8 > block.len() {
				break;
			}
			let rec = u16::from_le_bytes([block[off + 4], block[off + 5]]) as usize;
			let elen = block[off + 6] as usize;
			if rec < 8 {
				break;
			}
			let actual = ((8 + elen + 3) & !3) as usize;
			let next_off = off + rec;
			if next_off >= block.len() {
				if rec - actual >= needed as usize {
					block[off + 4..off + 6].copy_from_slice(&(actual as u16).to_le_bytes());
					let no = off + actual;
					let nr = (rec - actual) as u16;
					write_dir_entry(&mut block, no, child_ino, nr, nlen as u8, file_type, name_b);
					write_block(dev, sb, phys, &block);
					inserted = true;
				}
				break;
			}
			off = next_off;
		}
		if inserted {
			return;
		}
	}
	/* Allocate new block */
	let phys = match super::bitmap_alloc::alloc_block(dev, sb, bgdt) {
		Some(p) => p,
		None => return,
	};
	let bi = n_blks as u32;
	let mut block = alloc::vec![0u8; bsz];
	write_dir_entry(
		&mut block, 0, child_ino, bsz as u16, nlen as u8, file_type, name_b,
	);
	write_block(dev, sb, phys, &block);
	extent::set_block(dev, sb, bgdt, &mut dir_ino.block, bi, phys);
	dir_ino.size += bsz as u32;
	dir_ino.blocks += sb.sectors_per_block() as u32;
	dir_ino.write(dev, sb, bgdt);
}

pub fn remove_entry(
	dev: &dyn BlockDev,
	sb: &Superblock,
	bgdt: &BgDescTable,
	dir_ino: &Inode,
	name: &str,
) -> bool {
	let bsz = sb.block_size();
	let n_blks = (dir_ino.size() + bsz - 1) / bsz;
	for bi in 0..n_blks as u32 {
		let phys = match extent::get_block(dev, sb, &dir_ino.block, bi) {
			Some(p) => p,
			None => continue,
		};
		let mut block = read_block(dev, sb, phys);
		let mut prev_off: Option<usize> = None;
		let mut found = None;
		for_each_entry(&block, |cur, ino, rec, nb| {
			if ino != 0 && nb == name.as_bytes() {
				found = Some((cur, prev_off, rec));
				false
			} else {
				prev_off = Some(cur);
				true
			}
		});
		if let Some((cur, prev, rec)) = found {
			block[cur..cur + 4].copy_from_slice(&0u32.to_le_bytes());
			if let Some(poff) = prev {
				let pr = u16::from_le_bytes([block[poff + 4], block[poff + 5]]) as usize;
				block[poff + 4..poff + 6].copy_from_slice(&((pr + rec) as u16).to_le_bytes());
			}
			write_block(dev, sb, phys, &block);
			return true;
		}
	}
	false
}

fn write_dir_entry(b: &mut [u8], off: usize, ino: u32, rec: u16, nlen: u8, ft: u8, name: &[u8]) {
	b[off..off + 4].copy_from_slice(&ino.to_le_bytes());
	b[off + 4..off + 6].copy_from_slice(&rec.to_le_bytes());
	b[off + 6] = nlen;
	b[off + 7] = ft;
	b[off + 8..off + 8 + nlen as usize].copy_from_slice(&name[..nlen as usize]);
}

/*
 * rmdir - Remove an empty directory
 *
 * 1. Lookup child inode by name
 * 2. Verify child is a directory and empty (only . and ..)
 * 3. Free all data blocks of the child directory
 * 4. Decrement parent's link count and remove entry from parent
 * 5. Decrement child's link count (. entry) and free inode
 * 6. Update parent's used_dirs counter
 */
pub fn rmdir(
	dev: &dyn BlockDev,
	sb: &mut Superblock,
	bgdt: &mut BgDescTable,
	dir_ino: &mut Inode,
	name: &str,
) -> Result<(), &'static str> {
	let bsz = sb.block_size();

	/* 1. Lookup child inode */
	let child_ino = lookup_in_dir(dev, sb, bgdt, dir_ino, name).ok_or("entry not found")?;

	/* 2. Read child inode and verify it's a directory */
	let child = Inode::read(dev, sb, bgdt, child_ino).ok_or("cannot read child inode")?;
	if !child.is_dir() {
		return Err("not a directory");
	}

	/* 3. Check if directory is empty (only . and ..) */
	let n_blks = (child.size() + bsz - 1) / bsz;
	let mut entry_count = 0u32;
	for bi in 0..n_blks as u32 {
		let phys = match extent::get_block(dev, sb, &child.block, bi) {
			Some(p) => p,
			None => continue,
		};
		let block = read_block(dev, sb, phys);
		for_each_entry(&block, |_, ino, _, _| {
			if ino != 0 {
				entry_count += 1;
			}
			true
		});
	}
	if entry_count > 2 {
		return Err("directory not empty");
	}

	/* 4. Free all data blocks of the child directory */
	for bi in 0..n_blks as u32 {
		if let Some(phys) = extent::get_block(dev, sb, &child.block, bi) {
			super::bitmap_alloc::free_block(dev, sb, bgdt, phys);
		}
	}

	/* 5. Remove directory entry from parent */
	let parent_ino = dir_ino.ino;
	let parent = Inode::read(dev, sb, bgdt, parent_ino).ok_or("cannot read parent inode")?;
	if !remove_entry(dev, sb, bgdt, &parent, name) {
		return Err("failed to remove entry from parent");
	}

	/* 6. Decrement parent's link count */
	let mut parent_mut = Inode::read(dev, sb, bgdt, parent_ino).ok_or("cannot re-read parent")?;
	parent_mut.links_count = parent_mut.links_count.saturating_sub(1);
	parent_mut.write(dev, sb, bgdt);

	/* 7. Decrement child's link count (. entry) and free inode */
	let mut child_mut = Inode::read(dev, sb, bgdt, child_ino).ok_or("cannot re-read child")?;
	child_mut.links_count = child_mut.links_count.saturating_sub(1);
	child_mut.write(dev, sb, bgdt);
	super::bitmap_alloc::free_inode(dev, sb, bgdt, child_ino);

	/* 8. Update parent's used_dirs counter */
	let g = sb.inode_block_group(parent_ino) as usize;
	bgdt.get_mut(g).used_dirs = bgdt.get(g).used_dirs.saturating_sub(1);
	bgdt.write_entry(dev, sb, g);

	Ok(())
}
