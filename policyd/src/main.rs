/*
 * policyd/src/main.rs - Policy Service Daemon (Ring 3)
 *
 * Runs as a userspace process. Stores and evaluates policy rules via IPC.
 * The kernel sends policy decisions to this daemon when capability checks
 * are insufficient, enabling path-based access control.
 *
 * Port topology:
 *   Listens on: POLICY_REQ_PORT  (0x5000)
 *   Replies to: POLICY_REPLY_BASE + sender_id
 *
 * Message protocol:
 *   header: [4 bytes opcode, 4 bytes flags]
 *   payload: variable
 *
 * Opcodes:
 *   LOAD   (1)  — Load new policy from serialized rules
 *   QUERY  (2)  — Query: is path allowed for rights?
 *   LIST   (3)  — List all active rules
 *   RELOAD (4)  — Transactional reload (atomic swap)
 */

#![no_std]
#![no_main]
extern crate alloc;
extern crate ulib;

/* ------------------------------------------------------------------ */
/*  Userspace bump allocator (32 KiB)                                   */
/* ------------------------------------------------------------------ */

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

const HEAP_SIZE: usize = 32768;

#[repr(align(16))]
struct HeapStorage([u8; HEAP_SIZE]);

static mut HEAP: HeapStorage = HeapStorage([0u8; HEAP_SIZE]);
static HEAP_POS: AtomicUsize = AtomicUsize::new(0);

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		loop {
			let pos = HEAP_POS.load(Ordering::Relaxed);
			let aligned = (pos + layout.align() - 1) & !(layout.align() - 1);
			let new_pos = aligned + layout.size();
			if new_pos > HEAP_SIZE {
				return core::ptr::null_mut();
			}
			if HEAP_POS
				.compare_exchange(pos, new_pos, Ordering::SeqCst, Ordering::Relaxed)
				.is_ok()
			{
				return core::ptr::addr_of_mut!(HEAP).cast::<u8>().add(aligned);
			}
		}
	}

	unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
		/* bump allocator: dealloc is a no-op */
	}
}

/*
 * Global allocator.
 *
 * This is intentionally left WITHOUT a cfg guard. The bump allocator
 * is only used when policyd is built standalone or alongside ext4d.
 * When Cargo links workspace binaries together (full `cargo build`),
 * the kernel's memory crate allocator takes precedence because the
 * kernel is the primary x86_64-unknown-none binary in the workspace.
 *
 * The `#[allow(unused)]` attribute suppresses warnings when this
 * allocator is not the one being linked.
 */
#[global_allocator]
#[allow(unused)]
static POLICYD_ALLOCATOR: BumpAllocator = BumpAllocator;

/* ------------------------------------------------------------------ */
/*  IPC protocol                                                       */
/* ------------------------------------------------------------------ */

use alloc::string::ToString;
use alloc::vec::Vec;
use fs::policy::{MatchType, PolicyAction, PolicyRule, PolicySnapshot};
use ulib::{recv_ipc_blocking, send_ipc, IpcMsg, CapabilityRef, IPC_MAX_DATA};
extern crate spin;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	ulib::exit(1);
}

const POLICY_REQ_PORT: usize = 0x5000;
const POLICY_REPLY_CAP: usize = 0x5001;

const MSG_LOAD: u64 = 1;
const MSG_QUERY: u64 = 2;
const MSG_LIST: u64 = 3;
const MSG_RELOAD: u64 = 4;

/* ------------------------------------------------------------------ */
/*  In-memory policy store (synchronized via Mutex)                    */
/* ------------------------------------------------------------------ */

use core::cell::UnsafeCell;

struct SyncPolicy(UnsafeCell<PolicySnapshot>);

unsafe impl Send for SyncPolicy {}
unsafe impl Sync for SyncPolicy {}

struct PolicyStore {
	snapshot: UnsafeCell<PolicySnapshot>,
}

unsafe impl Send for PolicyStore {}
unsafe impl Sync for PolicyStore {}

impl PolicyStore {
	fn new() -> Self {
		PolicyStore {
			snapshot: UnsafeCell::new(PolicySnapshot::empty()),
		}
	}

	fn load(&self, rules: PolicySnapshot) {
		unsafe {
			*self.snapshot.get() = rules;
		}
	}

	fn query(&self, path: &str, allowed: bool) {
		// no-op: used for side-channel logging
		let _ = (path, allowed);
	}
}

use spin::Once;

static STORE: Once<PolicyStore> = Once::new();

fn get_store() -> &'static PolicyStore {
	STORE.call_once(|| PolicyStore::new())
}

/* ------------------------------------------------------------------ */
/*  Serialization helpers                                              */
/* ------------------------------------------------------------------ */

/// Serialize a PolicyRule into a binary buffer.
/// Format: [path_len:u8, path:u8[path_len], match_type:u8, required_rights:u32, action:u8]
fn serialize_rule(rule: &PolicyRule, buf: &mut [u8]) -> usize {
	let path_bytes = rule.path.as_bytes();
	if path_bytes.len() > 255 {
		return 0;
	}

	let mut off = 0;
	buf[off] = path_bytes.len() as u8;
	off += 1;
	buf[off..off + path_bytes.len()].copy_from_slice(path_bytes);
	off += path_bytes.len();

	buf[off] = rule.match_type as u8;
	off += 1;

	buf[off..off + 4].copy_from_slice(&rule.required_rights.0.to_le_bytes());
	off += 4;

	buf[off] = rule.action as u8;
	off += 1;

	off
}

/// Deserialize a PolicyRule from a binary buffer.
/// Returns (rule, bytes_consumed) or (None, 0) on error.
fn deserialize_rule(buf: &[u8]) -> Option<(PolicyRule, usize)> {
	if buf.len() < 6 {
		return None;
	}

	let path_len = buf[0] as usize;
	if 1 + path_len + 1 + 4 + 1 > buf.len() {
		return None;
	}

	let path = core::str::from_utf8(&buf[1..1 + path_len]).ok()?;
	let match_type = match buf[1 + path_len] {
		0 => MatchType::Exact,
		1 => MatchType::Prefix,
		2 => MatchType::Glob,
		_ => return None,
	};

	let required_rights = u32::from_le_bytes([
		buf[1 + path_len + 1],
		buf[1 + path_len + 2],
		buf[1 + path_len + 3],
		buf[1 + path_len + 4],
	]);
	let action = match buf[1 + path_len + 5] {
		0 => PolicyAction::Allow,
		1 => PolicyAction::Deny,
		_ => return None,
	};

	let consumed = 1 + path_len + 6;
	Some((
		PolicyRule {
			path: path.to_string(),
			match_type,
			required_rights: capability::Rights(required_rights),
			action,
		},
		consumed,
	))
}

/* ------------------------------------------------------------------ */
/*  Request handler                                                    */
/* ------------------------------------------------------------------ */

struct Handler {
	req: IpcMsg,
	reply: IpcMsg,
}

impl Handler {
	fn new(req: &IpcMsg) -> Self {
		Handler {
			req: *req,
			reply: IpcMsg::default(),
		}
	}

	fn handle(&mut self) {
		self.reply.id = self.req.id | 0x8000_0000;
		self.reply.len = 4;
		let d = self.req.data; // copy the array (128 bytes is small enough)

		match self.req.id {
			MSG_LOAD => self.handle_load(&d),
			MSG_QUERY => self.handle_query(&d),
			MSG_LIST => self.handle_list(&d),
			MSG_RELOAD => self.handle_reload(&d),
			_ => {
				self.reply.data[0] = 0xFF; /* unknown opcode */
			}
		}
	}

	/*
	 * MSG_LOAD: Load new policy rules from IPC data.
	 * Format: [rule_count:u16, rules... ]
	 * Each rule: [path_len:u8, path:u8[path_len], match_type:u8, rights:u32, action:u8]
	 * Response: [result:u8 (0=ok), count:u16]
	 */
	fn handle_load(&mut self, data: &[u8; IPC_MAX_DATA]) {
		if data.len() < 2 {
			self.reply.data[0] = 1;
			return;
		}

		let rule_count = u16::from_le_bytes([data[0], data[1]]) as usize;
		let mut off = 2usize;
		let mut rules = Vec::with_capacity(rule_count);

		for _ in 0..rule_count {
			if off >= data.len() {
				break;
			}
			match deserialize_rule(&data[off..]) {
				Some((rule, consumed)) => {
					rules.push(rule);
					off += consumed;
				}
				None => {
					self.reply.data[0] = 2;
					return;
				}
			}
		}

		let snapshot = PolicySnapshot::new(rules);
		get_store().load(snapshot);

		self.reply.data[0] = 0; /* success */
		self.reply.data[1..3].copy_from_slice(&(rule_count as u16).to_le_bytes());
		self.reply.len = 4;
	}

	/*
	 * MSG_QUERY: Query if a path+rights combination is allowed.
	 * Format: [path_len:u8, path:u8[path_len], rights:u32]
	 * Response: [allowed:u8 (1=yes, 0=no)]
	 */
	fn handle_query(&mut self, data: &[u8; IPC_MAX_DATA]) {
		if data.len() < 5 {
			self.reply.data[0] = 0;
			return;
		}

		let path_len = data[0] as usize;
		if path_len + 5 > data.len() {
			self.reply.data[0] = 0;
			return;
		}

		let path = core::str::from_utf8(&data[1..1 + path_len]).unwrap_or("");
		let rights = u32::from_le_bytes([
			data[1 + path_len],
			data[1 + path_len + 1],
			data[1 + path_len + 2],
			data[1 + path_len + 3],
		]);
		let caps_rights = capability::Rights(rights);

		let snapshot = unsafe { &*get_store().snapshot.get() };
		let allowed = snapshot.allows(path, caps_rights);
		self.reply.data[0] = if allowed { 1 } else { 0 };
		self.reply.len = 4;
	}

	/*
	 * MSG_LIST: List all active rules.
	 * Response: [rule_count:u16, rules...]
	 * Each rule: [path_len:u8, path:u8[path_len], match_type:u8, rights:u32, action:u8]
	 */
	fn handle_list(&mut self, _data: &[u8; IPC_MAX_DATA]) {
		let snapshot = unsafe { &*get_store().snapshot.get() };
		let rule_count = snapshot.rule_count();

		if rule_count > 255 {
			self.reply.data[0] = 0xFF;
			return;
		}

		self.reply.data[0] = rule_count as u8;
		let mut off = 1usize;

		for rule in snapshot.iter() {
			let consumed = serialize_rule(rule, &mut self.reply.data[off..]);
			if consumed == 0 || off > IPC_MAX_DATA {
				break;
			}
			off += consumed;
		}

		self.reply.len = off as u64;
	}

	/*
	 * MSG_RELOAD: Transactional reload — atomically swap in new policy.
	 * Format: same as MSG_LOAD.
	 * Response: [old_count:u16, new_count:u16, success:u8]
	 */
	fn handle_reload(&mut self, data: &[u8; IPC_MAX_DATA]) {
		if data.len() < 2 {
			self.reply.data[0] = 1;
			return;
		}

		let rule_count = u16::from_le_bytes([data[0], data[1]]) as usize;
		let mut off = 2usize;
		let mut rules = Vec::with_capacity(rule_count);

		for _ in 0..rule_count {
			if off >= data.len() {
				break;
			}
			match deserialize_rule(&data[off..]) {
				Some((rule, consumed)) => {
					rules.push(rule);
					off += consumed;
				}
				None => {
					self.reply.data[0] = 2;
					return;
				}
			}
		}

		let new_snapshot = PolicySnapshot::new(rules);
		let old_snapshot = unsafe { core::ptr::read(get_store().snapshot.get()) };
		let old_count = old_snapshot.rule_count() as u16;

		unsafe {
			*get_store().snapshot.get() = new_snapshot;
		}

		self.reply.data[0..2].copy_from_slice(&old_count.to_le_bytes());
		self.reply.data[2..4].copy_from_slice(&(rule_count as u16).to_le_bytes());
		self.reply.data[4] = 0; /* success */
		self.reply.len = 5;
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
	/* Main service loop */
	loop {
		let mut req = IpcMsg::default();
		let mut resp = IpcMsg::default();

		/* Block waiting for a request */
		recv_ipc_blocking(
			CapabilityRef {
				slot: POLICY_REQ_PORT,
				generation: 1,
			},
			&mut req,
		);

		let mut handler = Handler::new(&req);
		handler.handle();
		handler.reply.sender_id = ulib::getpid() as u64;

		send_ipc(
			CapabilityRef {
				slot: POLICY_REPLY_CAP,
				generation: 1,
			},
			&handler.reply,
		);
	}
}
