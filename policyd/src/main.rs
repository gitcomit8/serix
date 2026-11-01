/*
 * policyd/src/main.rs - Policy Service Daemon (Ring 3)
 *
 * Runs as a userspace process. Stores and evaluates policy rules via IPC.
 * The kernel sends policy decisions to this daemon when capability checks
 * are insufficient, enabling path-based access control.
 *
 * Port topology:
 *   Listens on: POLICY_REQ_PORT  (0x5000)
 *   Replies to: POLICY_REPLY_CAP (0x5001)
 *
 * Message protocol:
 *   header: [4 bytes opcode, 4 bytes flags]
 *   payload: variable
 *
 * Opcodes:
 *   LOAD   (1)  — Load new policy from serialized rules
 *   QUERY  (2)  — Query: is path allowed for rights?
 *   LIST   (3)  — List all active rules
 *   RELOAD (4)  — Reload policy from config file
 *
 * Config file (TOML):
 *   Loaded at startup from /etc/serix/policy.toml
 *   Hot-reload via RELOAD opcode (file polling) or kshell `policy reload`
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
 * Used when policyd is built standalone (no_std, no kernel).
 * When policyd is built alongside the kernel, the kernel's memory
 * crate allocator takes precedence — this one is ignored.
 */
#[allow(unused)]
#[global_allocator]
static POLICYD_ALLOCATOR: BumpAllocator = BumpAllocator;

/* ------------------------------------------------------------------ */
/*  Config parser module                                               */
/* ------------------------------------------------------------------ */

mod config;
use config::{config_rules_to_policy, parse_config};

/* ------------------------------------------------------------------ */
/*  IPC protocol                                                       */
/* ------------------------------------------------------------------ */

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use capability::Rights;
use fs::policy::{MatchType, PolicyAction, PolicyRule, PolicySnapshot};
use ulib::{recv_ipc_blocking, send_ipc, IpcMsg, CapabilityRef, IPC_MAX_DATA};
extern crate spin;

use core::cell::UnsafeCell;
use core::panic::PanicInfo;

static LAST_CONFIG_SIZE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

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

/* Config file path — used by RELOAD to find the source file */
const CONFIG_FILE_PATH: &str = "/etc/serix/policy.toml";

/* Config parent capability — slot 5 (after stdio 0-2, IPC caps 3-4) */
const CONFIG_PARENT_SLOT: usize = 5;
const CONFIG_PARENT_GEN: u32 = 1;

/* ------------------------------------------------------------------ */
/*  In-memory policy store (synchronized via Mutex)                    */
/* ------------------------------------------------------------------ */

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
		let mut rules = vec::Vec::with_capacity(rule_count);

		for _ in 0..rule_count {
			if off >= data.len() {
				break;
			}
			if off + 6 > data.len() {
				self.reply.data[0] = 2;
				return;
			}

			let path_len = data[off] as usize;
			if off + 1 + path_len + 5 > data.len() {
				self.reply.data[0] = 2;
				return;
			}

			let path = match core::str::from_utf8(&data[off + 1..off + 1 + path_len]) {
				Ok(s) => s.to_string(),
				Err(_) => {
					self.reply.data[0] = 2;
					return;
				}
			};

			let match_type = match data[off + 1 + path_len] {
				0 => MatchType::Exact,
				1 => MatchType::Prefix,
				2 => MatchType::Glob,
				_ => {
					self.reply.data[0] = 2;
					return;
				}
			};

			let required_rights = u32::from_le_bytes([
				data[off + 1 + path_len + 1],
				data[off + 1 + path_len + 2],
				data[off + 1 + path_len + 3],
				data[off + 1 + path_len + 4],
			]);
			let action = match data[off + 1 + path_len + 5] {
				0 => PolicyAction::Allow,
				1 => PolicyAction::Deny,
				_ => {
					self.reply.data[0] = 2;
					return;
				}
			};

			rules.push(PolicyRule {
				path,
				match_type,
				required_rights: Rights(required_rights),
				action,
			});

			off += 1 + path_len + 6;
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
		let caps_rights = Rights(rights);

		let snapshot = unsafe { &*get_store().snapshot.get() };
		let allowed = snapshot.allows(path, caps_rights);
		self.reply.data[0] = if allowed { 1 } else { 0 };
		self.reply.len = 4;
	}

	/*
	 * MSG_LIST: List all active rules.
	 * Response: [rule_count:u8, rules...]
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
			if consumed == 0 || off + consumed > IPC_MAX_DATA {
				break;
			}
			off += consumed;
		}

		self.reply.len = off as u64;
	}

	/*
	 * MSG_RELOAD: Reload policy from config file.
	 * Format: [config_path_len:u8, config_path:u8[path_path_len]]
	 *          or empty payload (use default CONFIG_FILE_PATH).
	 * Response: [result:u8 (0=ok, 1=file not found, 2=parse error),
	 *            old_count:u16, new_count:u16]
	 */
	fn handle_reload(&mut self, data: &[u8; IPC_MAX_DATA]) {
		let config_path = if data.len() >= 2 {
			let path_len = data[0] as usize;
			if path_len > 0 && 1 + path_len <= data.len() {
				core::str::from_utf8(&data[1..1 + path_len]).unwrap_or(CONFIG_FILE_PATH)
			} else {
				CONFIG_FILE_PATH
			}
		} else {
			CONFIG_FILE_PATH
		};

		/* Read and parse the config file */
		let mut buf = [0u8; 4096];
		let n = unsafe {
			syscall_file_read(CONFIG_PARENT_SLOT, CONFIG_PARENT_GEN as usize, config_path, &mut buf)
		};

		if n == 0 {
			self.reply.data[0] = 1; /* file not found */
			let old_count = unsafe { (&*get_store().snapshot.get()).rule_count() } as u16;
			self.reply.data[1..3].copy_from_slice(&old_count.to_le_bytes());
			self.reply.len = 3;
			return;
		}

		let text = match core::str::from_utf8(&buf[..n as usize]) {
			Ok(s) => s,
			Err(_) => {
				self.reply.data[0] = 2; /* parse error */
				self.reply.len = 3;
				return;
			}
		};

		let config_rules = parse_config(text);
		if config_rules.is_empty() {
			self.reply.data[0] = 2; /* parse error — no rules */
			self.reply.len = 3;
			return;
		}

		/* Convert config rules to policy rules */
		let rules = config_rules_to_policy(&config_rules);

		let old_snapshot = unsafe { core::ptr::read(get_store().snapshot.get()) };
		let old_count = old_snapshot.rule_count() as u16;

		let new_snapshot = PolicySnapshot::new(rules);
		let new_count = new_snapshot.rule_count() as u16;

		unsafe {
			*get_store().snapshot.get() = new_snapshot;
		}

		self.reply.data[0] = 0; /* success */
		self.reply.data[1..3].copy_from_slice(&old_count.to_le_bytes());
		self.reply.data[3..5].copy_from_slice(&new_count.to_le_bytes());
		self.reply.len = 5;
	}
}

/* ------------------------------------------------------------------ */
/*  Direct syscall wrapper (no_std userspace)                         */
/* ------------------------------------------------------------------ */

/// Direct syscall wrapper for SYS_FILE_READ (92).
/// Used to read config files at boot and during hot-reload.
unsafe fn syscall_file_read(cap_slot: usize, cap_gen: usize, path: &str, buf: &mut [u8]) -> usize {
	let ret: usize;
	core::arch::asm!(
		"syscall",
		in("rax") 92u64,
		in("rdi") cap_slot,
		in("rsi") cap_gen,
		in("rdx") path.as_ptr() as usize,
		in("r10") path.len(),
		in("r8") buf.as_mut_ptr() as usize,
		in("r9") buf.len(),
		out("rcx") _,
		out("r11") _,
		lateout("rax") ret,
		options(nostack),
	);
	ret
}

/* ------------------------------------------------------------------ */
/*  Entry point                                                        */
/* ------------------------------------------------------------------ */

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	main();
}

fn main() -> ! {
	/* Load config from file at startup */
	let mut buf = [0u8; 4096];
	let n = unsafe {
		syscall_file_read(CONFIG_PARENT_SLOT, CONFIG_PARENT_GEN as usize, CONFIG_FILE_PATH, &mut buf)
	};

	if n > 0 {
		if let Ok(text) = core::str::from_utf8(&buf[..n as usize]) {
			let config_rules = parse_config(text);
			let rules = config_rules_to_policy(&config_rules);
			if !rules.is_empty() {
				get_store().load(PolicySnapshot::new(rules));
			}
		}
	}

	/* Main service loop with file polling for hot-reload */
	LAST_CONFIG_SIZE.store(n as usize, core::sync::atomic::Ordering::Relaxed);

	loop {
		/* Check config file size for changes (simple polling) */
		if check_config_changed(CONFIG_FILE_PATH) {
			if let Ok(rules) = load_and_parse_config() {
				get_store().load(PolicySnapshot::new(rules));
			}
		}

		/* Block waiting for a request */
		let mut req = IpcMsg::default();
		let mut resp = IpcMsg::default();

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

/* ------------------------------------------------------------------ */
/*  Hot-reload helpers                                                 */
/* ------------------------------------------------------------------ */

/// Check if the config file has changed by comparing its size.
fn check_config_changed(path: &str) -> bool {
	let mut buf = [0u8; 1];
	let n = unsafe { syscall_file_read(CONFIG_PARENT_SLOT, CONFIG_PARENT_GEN as usize, path, &mut buf) };

	if n != 0 {
		let current_size = n as usize;
		let old_size = LAST_CONFIG_SIZE.load(core::sync::atomic::Ordering::Relaxed);
		let changed = current_size != old_size;
		LAST_CONFIG_SIZE.store(current_size, core::sync::atomic::Ordering::Relaxed);
		changed
	} else {
		false
	}
}

/// Load and parse the config file, returning policy rules.
fn load_and_parse_config() -> Result<Vec<PolicyRule>, ()> {
	let mut buf = [0u8; 4096];
	let n = unsafe { syscall_file_read(CONFIG_PARENT_SLOT, CONFIG_PARENT_GEN as usize, CONFIG_FILE_PATH, &mut buf) };

	if n == 0 {
		return Err(());
	}

	let text = core::str::from_utf8(&buf[..n as usize]).map_err(|_| ())?;
	let config_rules = parse_config(text);
	let rules = config_rules_to_policy(&config_rules);

	if rules.is_empty() {
		Err(())
	} else {
		Ok(rules)
	}
}
