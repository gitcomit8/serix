/*
 * ext4d/src/main.rs - ext4 Filesystem Daemon (Ring 3)
 *
 * Runs as a userspace process. Opens /dev/sda, parses the ext4 filesystem,
 * and serves VFS operations forwarded by the kernel stub via IPC.
 *
 * Port topology:
 *   Listens on:  EXT4_REQ_PORT  (0x4E00)
 *   Replies to:  EXT4_REPLY_BASE + sender_id (kernel always uses +0)
 */

#![no_std]
#![no_main]
extern crate alloc;
extern crate ulib;

use alloc::vec::Vec;
use ulib::{read, write, open, IpcMsg, send_ipc, recv_ipc_blocking, IPC_MAX_DATA};
#[allow(unused_imports)]
use fs::BlockDev;
use fs::ext4::{
	ipc as proto,
	superblock::Superblock,
	bgdt::BgDescTable,
	inode::Inode,
	extent,
	dir,
	bitmap_alloc,
};

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	ulib::exit(-1);
}

/* ------------------------------------------------------------------ */
/*  Block device adapter: fd → BlockDev                               */
/* ------------------------------------------------------------------ */

struct FdBlockDev { fd: usize }

impl fs::BlockDev for FdBlockDev {
	fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
		let offset = (sector * 512) as usize;
		ulib::seek(self.fd, offset);
		read(self.fd, buf) == 512
	}

	fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool {
		let offset = (sector * 512) as usize;
		ulib::seek(self.fd, offset);
		write(self.fd, buf) == 512isize
	}

	fn sector_count(&self) -> u64 { u64::MAX }
}

/* ------------------------------------------------------------------ */
/*  Request handler                                                    */
/* ------------------------------------------------------------------ */

struct State<'a> {
	dev:  &'a FdBlockDev,
	sb:   Superblock,
	bgdt: BgDescTable,
}

/*
 * handle - Dispatch one IPC request and fill the reply message
 * @state: mutable filesystem state
 * @req:   incoming request message
 * @reply: reply message to populate
 */
fn handle(state: &mut State, req: &IpcMsg, reply: &mut IpcMsg) {
	reply.id  = req.id | 0x8000_0000;
	reply.len = 4;
	let d = &req.data;

	match req.id {
		proto::MSG_LOOKUP => {
			let parent_ino = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let nlen = d[4] as usize;
			/* name starts at d[5] (mk_req: [0..4]=ino, [4]=nlen, [5..]=name) */
			let nlen_clamped = nlen.min(112);
			let name = core::str::from_utf8(&d[5..5 + nlen_clamped]).unwrap_or("");
			let parent = match Inode::read(state.dev, &state.sb, &state.bgdt, parent_ino) {
				Some(i) => i,
				None => { reply.data[4] = 0; return; }
			};
			match dir::lookup_in_dir(state.dev, &state.sb, &state.bgdt, &parent, name) {
				Some(child_ino) => {
					reply.data[0..4].copy_from_slice(&child_ino.to_le_bytes());
					let child = Inode::read(state.dev, &state.sb, &state.bgdt, child_ino);
					reply.data[4] = child.map(|i| if i.is_dir() { 2u8 } else { 1u8 }).unwrap_or(0);
				}
				None => {
					reply.data[0..4].copy_from_slice(&0u32.to_le_bytes());
					reply.data[4] = 0;
				}
			}
		}

		proto::MSG_STAT => {
			let ino = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			if let Some(inode) = Inode::read(state.dev, &state.sb, &state.bgdt, ino) {
				let sz = inode.size() as u64;
				reply.data[0..8].copy_from_slice(&sz.to_le_bytes());
				reply.data[8] = if inode.is_dir() { 1 } else { 0 };
				reply.len = 9;
			}
		}

		proto::MSG_SIZE => {
			let ino = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let sz = Inode::read(state.dev, &state.sb, &state.bgdt, ino)
				.map(|i| i.size() as u32)
				.unwrap_or(0);
			reply.data[0..4].copy_from_slice(&sz.to_le_bytes());
		}

		proto::MSG_READ => {
			let ino     = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let offset  = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize;
			let max_len = u16::from_le_bytes([d[8], d[9]]) as usize;
			let max_len = max_len.min(proto::MAX_DATA);
			let inode = match Inode::read(state.dev, &state.sb, &state.bgdt, ino) {
				Some(i) => i,
				None => {
					reply.data[0..2].copy_from_slice(&0u16.to_le_bytes());
					return;
				}
			};
			let file_size = inode.size();
			if offset >= file_size {
				reply.data[0..2].copy_from_slice(&0u16.to_le_bytes());
				return;
			}
			let bsz     = state.sb.block_size();
			let to_read = max_len.min(file_size - offset);
			let blk_idx = (offset / bsz) as u32;
			let blk_off = offset % bsz;
			let phys = match extent::get_block(state.dev, &state.sb, &inode.block, blk_idx) {
				Some(p) => p,
				None => {
					reply.data[0..2].copy_from_slice(&0u16.to_le_bytes());
					return;
				}
			};
			/* Read the full block, copy the requested slice */
			let mut blk_buf = alloc::vec![0u8; bsz];
			let spb = state.sb.sectors_per_block();
			let sec = state.sb.block_to_sector(phys as u64);
			let mut off2 = 0usize;
			for s in 0..spb as usize {
				let mut sec_buf = [0u8; 512];
				state.dev.read_block(sec + s as u64, &mut sec_buf);
				blk_buf[off2..off2 + 512].copy_from_slice(&sec_buf);
				off2 += 512;
			}
			let avail = (bsz - blk_off).min(to_read);
			reply.data[2..2 + avail].copy_from_slice(&blk_buf[blk_off..blk_off + avail]);
			reply.data[0..2].copy_from_slice(&(avail as u16).to_le_bytes());
			reply.len = (2 + avail) as u64;
		}

		proto::MSG_WRITE => {
			let ino    = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let offset = u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize;
			let len    = u16::from_le_bytes([d[8], d[9]]) as usize;
			let len    = len.min(proto::MAX_DATA);
			let src    = &d[10..10 + len];
			let mut inode = match Inode::read(state.dev, &state.sb, &state.bgdt, ino) {
				Some(i) => i,
				None => {
					reply.data[0..4].copy_from_slice(&0u32.to_le_bytes());
					return;
				}
			};
			let bsz     = state.sb.block_size();
			let blk_idx = (offset / bsz) as u32;
			let blk_off = offset % bsz;
			let phys = extent::get_block(state.dev, &state.sb, &inode.block, blk_idx)
				.unwrap_or_else(|| {
					let p = bitmap_alloc::alloc_block(
						state.dev, &mut state.sb, &mut state.bgdt,
					).unwrap_or(0);
					let zeros = alloc::vec![0u8; bsz];
					let spb = state.sb.sectors_per_block();
					let sec = state.sb.block_to_sector(p as u64);
					let mut o = 0usize;
					for s in 0..spb as usize {
						let mut sb = [0u8; 512];
						sb.copy_from_slice(&zeros[o..o + 512]);
						state.dev.write_block(sec + s as u64, &sb);
						o += 512;
					}
					extent::set_block(
						state.dev, &mut state.sb, &mut state.bgdt,
						&mut inode.block, blk_idx, p,
					);
					inode.blocks += state.sb.sectors_per_block() as u32;
					p
				});
			/* Read-modify-write the target block */
			let mut blk_buf = alloc::vec![0u8; bsz];
			let spb = state.sb.sectors_per_block();
			let sec = state.sb.block_to_sector(phys as u64);
			let mut o = 0usize;
			for s in 0..spb as usize {
				let mut sb = [0u8; 512];
				state.dev.read_block(sec + s as u64, &mut sb);
				blk_buf[o..o + 512].copy_from_slice(&sb);
				o += 512;
			}
			let avail = (bsz - blk_off).min(len);
			blk_buf[blk_off..blk_off + avail].copy_from_slice(&src[..avail]);
			let mut o = 0usize;
			for s in 0..spb as usize {
				let mut sb = [0u8; 512];
				sb.copy_from_slice(&blk_buf[o..o + 512]);
				state.dev.write_block(sec + s as u64, &sb);
				o += 512;
			}
			let new_end = offset + avail;
			if new_end > inode.size() { inode.size = new_end as u32; }
			inode.write(state.dev, &state.sb, &state.bgdt);
			reply.data[0..4].copy_from_slice(&(avail as u32).to_le_bytes());
		}

		proto::MSG_READDIR => {
			let ino  = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let skip = u16::from_le_bytes([d[4], d[5]]) as usize;
			let inode = match Inode::read(state.dev, &state.sb, &state.bgdt, ino) {
				Some(i) => i,
				None => { reply.data[0] = 0; return; }
			};
			let all = dir::readdir(state.dev, &state.sb, &state.bgdt, &inode);
			let slice = if skip < all.len() { &all[skip..] } else { &[] };
			let mut out_off = 1usize;
			let mut count   = 0u8;
			for (name, ftype) in slice {
				let nb   = name.as_bytes();
				let nlen = nb.len();
				if out_off + 6 + nlen > IPC_MAX_DATA { break; }
				/* ino (unused in response) */
				reply.data[out_off..out_off + 4].copy_from_slice(&0u32.to_le_bytes());
				reply.data[out_off + 4] = if *ftype == vfs::FileType::Directory { 2 } else { 1 };
				reply.data[out_off + 5] = nlen as u8;
				reply.data[out_off + 6..out_off + 6 + nlen].copy_from_slice(nb);
				out_off += 6 + nlen;
				count   += 1;
			}
			reply.data[0] = count;
			reply.len     = out_off as u64;
		}

		proto::MSG_MKDIR => {
			let parent_ino = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let nlen = d[4] as usize;
			let nlen_clamped = nlen.min(112);
			let name = core::str::from_utf8(&d[5..5 + nlen_clamped]).unwrap_or("");
			let child_ino = match bitmap_alloc::alloc_inode(
				state.dev, &mut state.sb, &mut state.bgdt,
			) {
				Some(i) => i,
				None => { reply.data[0..4].copy_from_slice(&0u32.to_le_bytes()); return; }
			};
			let blk = match bitmap_alloc::alloc_block(
				state.dev, &mut state.sb, &mut state.bgdt,
			) {
				Some(b) => b,
				None => { reply.data[0..4].copy_from_slice(&0u32.to_le_bytes()); return; }
			};
			let bsz = state.sb.block_size();
			let spb = state.sb.sectors_per_block();
			let mut dir_blk = alloc::vec![0u8; bsz];
			dir_blk[0..4].copy_from_slice(&child_ino.to_le_bytes());
			dir_blk[4..6].copy_from_slice(&12u16.to_le_bytes());
			dir_blk[6] = 1;
			dir_blk[7] = dir::EXT4_FT_DIR;
			dir_blk[8] = b'.';
			let rem = (bsz - 12) as u16;
			dir_blk[12..16].copy_from_slice(&parent_ino.to_le_bytes());
			dir_blk[16..18].copy_from_slice(&rem.to_le_bytes());
			dir_blk[18] = 2;
			dir_blk[19] = dir::EXT4_FT_DIR;
			dir_blk[20] = b'.';
			dir_blk[21] = b'.';
			let sec = state.sb.block_to_sector(blk as u64);
			let mut o = 0usize;
			for s in 0..spb as usize {
				let mut sb = [0u8; 512];
				sb.copy_from_slice(&dir_blk[o..o + 512]);
				state.dev.write_block(sec + s as u64, &sb);
				o += 512;
			}
			let child_block = [0u32; 15];
			use fs::ext4::inode::{EXT4_S_IFDIR, EXT4_EXTENTS_FL};
			let mut child = Inode {
				mode: EXT4_S_IFDIR | 0o755,
				links_count: 2,
				size: bsz as u32,
				blocks: spb as u32,
				flags: EXT4_EXTENTS_FL,
				block: child_block,
				ino: child_ino,
			};
			extent::set_block(
				state.dev, &mut state.sb, &mut state.bgdt,
				&mut child.block, 0, blk,
			);
			child.write(state.dev, &state.sb, &state.bgdt);
			let mut parent = match Inode::read(
				state.dev, &state.sb, &state.bgdt, parent_ino,
			) {
				Some(i) => i,
				None => { reply.data[0..4].copy_from_slice(&0u32.to_le_bytes()); return; }
			};
			dir::add_entry(
				state.dev, &mut state.sb, &mut state.bgdt,
				&mut parent, name, child_ino, dir::EXT4_FT_DIR,
			);
			let g = state.sb.inode_block_group(parent_ino) as usize;
			state.bgdt.get_mut(g).used_dirs += 1;
			state.bgdt.write_entry(state.dev, &state.sb, g);
			reply.data[0..4].copy_from_slice(&child_ino.to_le_bytes());
		}

		proto::MSG_CREATE => {
			let parent_ino = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let nlen = d[4] as usize;
			let nlen_clamped = nlen.min(112);
			let name = core::str::from_utf8(&d[5..5 + nlen_clamped]).unwrap_or("");
			let child_ino = match bitmap_alloc::alloc_inode(
				state.dev, &mut state.sb, &mut state.bgdt,
			) {
				Some(i) => i,
				None => { reply.data[0..4].copy_from_slice(&0u32.to_le_bytes()); return; }
			};
			use fs::ext4::inode::{EXT4_S_IFREG, EXT4_EXTENTS_FL};
			let child = Inode {
				mode: EXT4_S_IFREG | 0o644,
				links_count: 1,
				size: 0,
				blocks: 0,
				flags: EXT4_EXTENTS_FL,
				block: [0u32; 15],
				ino: child_ino,
			};
			child.write(state.dev, &state.sb, &state.bgdt);
			let mut parent = match Inode::read(
				state.dev, &state.sb, &state.bgdt, parent_ino,
			) {
				Some(i) => i,
				None => { reply.data[0..4].copy_from_slice(&0u32.to_le_bytes()); return; }
			};
			dir::add_entry(
				state.dev, &mut state.sb, &mut state.bgdt,
				&mut parent, name, child_ino, dir::EXT4_FT_REG_FILE,
			);
			reply.data[0..4].copy_from_slice(&child_ino.to_le_bytes());
		}

		proto::MSG_UNLINK => {
			let parent_ino = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
			let nlen = d[4] as usize;
			let nlen_clamped = nlen.min(112);
			let name = core::str::from_utf8(&d[5..5 + nlen_clamped]).unwrap_or("");
			let parent = match Inode::read(state.dev, &state.sb, &state.bgdt, parent_ino) {
				Some(i) => i,
				None => { reply.data[0] = 1; return; }
			};
			let child_ino = match dir::lookup_in_dir(
				state.dev, &state.sb, &state.bgdt, &parent, name,
			) {
				Some(i) => i,
				None => { reply.data[0] = 1; return; }
			};
			let child = match Inode::read(state.dev, &state.sb, &state.bgdt, child_ino) {
				Some(i) => i,
				None => { reply.data[0] = 1; return; }
			};
			let bsz    = state.sb.block_size();
			let n_blks = (child.size() + bsz - 1) / bsz;
			for b in 0..n_blks as u32 {
				if let Some(phys) = extent::get_block(state.dev, &state.sb, &child.block, b) {
					bitmap_alloc::free_block(
						state.dev, &mut state.sb, &mut state.bgdt, phys,
					);
				}
			}
			bitmap_alloc::free_inode(state.dev, &mut state.sb, &mut state.bgdt, child_ino);
			let parent2 = Inode::read(
				state.dev, &state.sb, &state.bgdt, parent_ino,
			).unwrap();
			dir::remove_entry(state.dev, &state.sb, &state.bgdt, &parent2, name);
			reply.data[0] = 0;
		}

		_ => { reply.data[0] = 0xFF; } /* unknown request */
	}
}

/* ------------------------------------------------------------------ */
/*  Entry point                                                        */
/* ------------------------------------------------------------------ */

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	main();
}

fn main() -> ! {
	/* Open /dev/sda */
	let fd = open(b"/dev/sda\0", 0);
	/* Large values indicate error (kernel returns ERRNO_ENOENT etc.) */
	if fd > isize::MAX as usize {
		ulib::exit(1);
	}
	let dev = FdBlockDev { fd };

	/* Parse ext4 superblock */
	let sb = match Superblock::read(&dev) {
		Some(s) => s,
		None => ulib::exit(2),
	};
	let bgdt = BgDescTable::read(&dev, &sb);
	let mut state = State { dev: &dev, sb, bgdt };

	/* Main service loop */
	loop {
		let mut req  = IpcMsg::default();
		let mut resp = IpcMsg::default();
		recv_ipc_blocking(proto::EXT4_REQ_PORT, &mut req);
		handle(&mut state, &req, &mut resp);
		resp.sender_id = ulib::getpid() as u64;
		let reply_port = proto::EXT4_REPLY_BASE + req.sender_id;
		send_ipc(reply_port, &resp);
	}
}
