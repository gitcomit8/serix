/*
 * ext4/kernel_stub.rs - VFS INode stubs that proxy to the ext4 daemon
 *
 * Every INode operation serialises a request into an ipc::Message, sends it
 * to EXT4_REQ_PORT, and blocks on a per-task reply port until the daemon
 * responds. The reply port ID is (EXT4_REPLY_BASE + kernel_task_id).
 *
 * Because kshell and other kernel tasks have no meaningful task_id visible
 * here, we use a fixed kernel reply port (EXT4_REPLY_BASE + 0) and serialise
 * all VFS calls through a single mutex. This is correct for single-threaded
 * shell usage; a real implementation would use per-task reply ports.
 */

extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use vfs::{FileType, INode};
use ipc::{IPC_GLOBAL, Message};
use super::ipc as proto;

/* Expose request port value for mod.rs */
pub const EXT4_REQ_PORT_VAL: u64 = proto::EXT4_REQ_PORT;

/* One global lock so we don't interleave requests on the kernel reply port */
static STUB_LOCK: Mutex<()> = Mutex::new(());

const KERNEL_REPLY_PORT: u64 = proto::EXT4_REPLY_BASE; /* +0 for kernel tasks */

fn send_and_recv(req: Message) -> Message {
	let _g = STUB_LOCK.lock();
	/* Ensure kernel reply port exists */
	{
		if IPC_GLOBAL.get_port(KERNEL_REPLY_PORT).is_none() {
			IPC_GLOBAL.create_port(KERNEL_REPLY_PORT);
		}
	}
	/* Drain any stale reply messages */
	while let Some(p) = IPC_GLOBAL.get_port(KERNEL_REPLY_PORT) {
		if p.receive().is_none() { break; }
	}
	if let Some(port) = IPC_GLOBAL.get_port(proto::EXT4_REQ_PORT) {
		port.send(req);
	}
	IPC_GLOBAL.get_port(KERNEL_REPLY_PORT)
		.map(|p| p.receive_blocking())
		.unwrap_or_default()
}

fn mk_req(id: u64, ino: u32, data: &[u8]) -> Message {
	let mut msg = Message { id, ..Default::default() };
	msg.data[0..4].copy_from_slice(&ino.to_le_bytes());
	let n = data.len().min(ipc::MAX_MSG_SIZE - 4);
	msg.data[4..4 + n].copy_from_slice(&data[..n]);
	msg.len = (4 + n) as u64;
	msg
}

/* ------------------------------------------------------------------ */

pub struct Ext4FileStub { pub ino: u32 }
pub struct Ext4DirStub  { pub ino: u32 }

impl INode for Ext4FileStub {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		let mut done = 0usize;
		while done < buf.len() {
			let max = (buf.len() - done).min(proto::MAX_DATA) as u16;
			let mut d = [0u8; 8];
			d[0..4].copy_from_slice(&((offset + done) as u32).to_le_bytes());
			d[4..6].copy_from_slice(&max.to_le_bytes());
			let resp = send_and_recv(mk_req(proto::MSG_READ, self.ino, &d));
			let actual = u16::from_le_bytes([resp.data[0], resp.data[1]]) as usize;
			if actual == 0 { break; }
			buf[done..done + actual].copy_from_slice(&resp.data[2..2 + actual]);
			done += actual;
		}
		done
	}

	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		let mut done = 0usize;
		while done < buf.len() {
			let chunk = (buf.len() - done).min(proto::MAX_DATA);
			let mut d = [0u8; 6 + proto::MAX_DATA];
			d[0..4].copy_from_slice(&((offset + done) as u32).to_le_bytes());
			d[4..6].copy_from_slice(&(chunk as u16).to_le_bytes());
			d[6..6 + chunk].copy_from_slice(&buf[done..done + chunk]);
			let resp = send_and_recv(mk_req(proto::MSG_WRITE, self.ino, &d));
			let written = u32::from_le_bytes([
				resp.data[0], resp.data[1], resp.data[2], resp.data[3],
			]) as usize;
			if written == 0 { break; }
			done += written;
		}
		done
	}

	fn metadata(&self) -> FileType { FileType::File }

	fn size(&self) -> usize {
		let resp = send_and_recv(mk_req(proto::MSG_SIZE, self.ino, &[]));
		u32::from_le_bytes([resp.data[0], resp.data[1], resp.data[2], resp.data[3]]) as usize
	}
}

impl INode for Ext4DirStub {
	fn read(&self, _: usize, _: &mut [u8]) -> usize { 0 }
	fn write(&self, _: usize, _: &[u8]) -> usize { 0 }
	fn metadata(&self) -> FileType { FileType::Directory }

	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>> {
		let nb = name.as_bytes();
		let nlen = nb.len().min(112);
		let mut d = [0u8; 1 + 112];
		d[0] = nlen as u8;
		d[1..1 + nlen].copy_from_slice(&nb[..nlen]);
		let resp = send_and_recv(mk_req(proto::MSG_LOOKUP, self.ino, &d));
		let child_ino = u32::from_le_bytes([
			resp.data[0], resp.data[1], resp.data[2], resp.data[3],
		]);
		let ftype = resp.data[4];
		if child_ino == 0 { return None; }
		if ftype == 2 {
			Some(Arc::new(Ext4DirStub  { ino: child_ino }))
		} else {
			Some(Arc::new(Ext4FileStub { ino: child_ino }))
		}
	}

	fn readdir(&self) -> Option<Vec<(String, FileType)>> {
		let mut entries = Vec::new();
		let mut skip: u16 = 0;
		loop {
			let mut d = [0u8; 2];
			d[0..2].copy_from_slice(&skip.to_le_bytes());
			let resp = send_and_recv(mk_req(proto::MSG_READDIR, self.ino, &d));
			let count = resp.data[0] as usize;
			if count == 0 { break; }
			let mut off = 1usize;
			for _ in 0..count {
				if off + 6 > ipc::MAX_MSG_SIZE { break; }
				let _ino = u32::from_le_bytes([
					resp.data[off], resp.data[off + 1],
					resp.data[off + 2], resp.data[off + 3],
				]);
				let ft   = resp.data[off + 4];
				let nlen = resp.data[off + 5] as usize;
				off += 6;
				if off + nlen > ipc::MAX_MSG_SIZE { break; }
				let name = String::from(
					core::str::from_utf8(&resp.data[off..off + nlen]).unwrap_or("?"),
				);
				let ftype = if ft == 2 { FileType::Directory } else { FileType::File };
				entries.push((name, ftype));
				off += nlen;
				skip += 1;
			}
		}
		Some(entries)
	}

	fn mkdir(&self, name: &str) -> Result<(), &'static str> {
		let nb = name.as_bytes();
		let nlen = nb.len().min(112);
		let mut d = [0u8; 1 + 112];
		d[0] = nlen as u8;
		d[1..1 + nlen].copy_from_slice(&nb[..nlen]);
		let resp = send_and_recv(mk_req(proto::MSG_MKDIR, self.ino, &d));
		let child = u32::from_le_bytes([
			resp.data[0], resp.data[1], resp.data[2], resp.data[3],
		]);
		if child == 0 { Err("mkdir failed") } else { Ok(()) }
	}

	fn create_file(&self, name: &str) -> Result<Arc<dyn INode>, &'static str> {
		let nb = name.as_bytes();
		let nlen = nb.len().min(112);
		let mut d = [0u8; 1 + 112];
		d[0] = nlen as u8;
		d[1..1 + nlen].copy_from_slice(&nb[..nlen]);
		let resp = send_and_recv(mk_req(proto::MSG_CREATE, self.ino, &d));
		let child_ino = u32::from_le_bytes([
			resp.data[0], resp.data[1], resp.data[2], resp.data[3],
		]);
		if child_ino == 0 { return Err("create failed"); }
		Ok(Arc::new(Ext4FileStub { ino: child_ino }))
	}

	fn unlink(&self, name: &str) -> Result<(), &'static str> {
		let nb = name.as_bytes();
		let nlen = nb.len().min(112);
		let mut d = [0u8; 1 + 112];
		d[0] = nlen as u8;
		d[1..1 + nlen].copy_from_slice(&nb[..nlen]);
		let resp = send_and_recv(mk_req(proto::MSG_UNLINK, self.ino, &d));
		if resp.data[0] == 0 { Ok(()) } else { Err("unlink failed") }
	}

	fn insert(&self, _: &str, _: Arc<dyn INode>) -> Result<(), &'static str> {
		Err("use create_file or mkdir for ext4 directories")
	}
}
