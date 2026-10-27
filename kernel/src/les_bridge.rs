/*
 * kernel/src/les_bridge.rs - Least-Elevated Security (LES) bridge
 *
 * Integrates the declarative policy engine into the kernel's syscall
 * authorization path. Provides three mechanisms:
 *
 * 1. **Local policy snapshot** — fast path evaluation without IPC.
 *    Loaded at boot with a minimal set of deny rules (e.g., /etc/shadow).
 *
 * 2. **Policy daemon fallback** — when the local snapshot lacks a rule
 *    for the path, the kernel forwards a QUERY request to the Ring-3
 *    policy service (policyd) on port 0x5000 via the IPC subsystem.
 *
 * 3. **Transactional policy reload** — atomic swap of the local policy
 *    snapshot so running syscalls are never exposed to a partially-updated
 *    policy.
 *
 * Usage:
 *   les_bridge::evaluate(path, rights) -> bool
 *   les_bridge::set_policy(rules)       // transactional reload
 */

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use capability::Rights;
use fs::policy::{PolicyAction, PolicyRule, PolicySnapshot};

/* ------------------------------------------------------------------ */
/*  Local policy store (atomic swap)                                   */
/* ------------------------------------------------------------------ */

/// Atomic policy pointer.
///
/// Stores a `None` initially (no policy loaded). `evaluate()` falls back
/// to the policy daemon when the local snapshot is empty.
static LOCAL_POLICY: AtomicUsize = AtomicUsize::new(0);

/// Load a policy snapshot into the local store.
///
/// This is an atomic swap — running syscalls that hold a reference to the
/// old policy are unaffected.
pub fn set_policy(rules: Vec<PolicyRule>) {
	let new_snapshot = Box::into_raw(Box::new(PolicySnapshot::new(rules))) as usize;
	let old = LOCAL_POLICY.swap(new_snapshot, Ordering::AcqRel);

	// Drop the old policy if it was non-null.
	if old != 0 {
		unsafe {
			drop(Box::from_raw(old as *mut PolicySnapshot));
		}
	}
}

/// Get a clone of the current local policy snapshot.
/// Returns `None` if no policy has been loaded.
fn get_local_policy() -> Option<PolicySnapshot> {
	let ptr = LOCAL_POLICY.load(Ordering::Relaxed);
	if ptr == 0 {
		None
	} else {
		unsafe { Some((ptr as *const PolicySnapshot).as_ref()?.clone()) }
	}
}

/* ------------------------------------------------------------------ */
/*  Policy daemon IPC                                                  */
/* ------------------------------------------------------------------ */

use ipc::Message;

/// Build a QUERY request message for the policy daemon.
/// Format: [opcode:u8, path_len:u8, path:u8[path_len], rights:u32]
fn build_query_msg(path: &str, rights: Rights) -> Message {
	let path_bytes = path.as_bytes();
	let data_len = 2 + path_bytes.len() + 4; // opcode + path_len + path + rights
	let mut data = [0u8; 128];
	data[0] = 2; // MSG_QUERY
	data[1] = path_bytes.len() as u8;
	data[2..2 + path_bytes.len()].copy_from_slice(path_bytes);
	data[2 + path_bytes.len()..2 + path_bytes.len() + 4]
		.copy_from_slice(&rights.0.to_le_bytes());

	Message {
		sender_id: 0,
		id: 2, // MSG_QUERY
		len: data_len as u64,
		data,
	}
}

/// Query the policy daemon for a path+rights decision.
///
/// Sends a QUERY message to the policy daemon's request port and waits
/// for a response. Returns true if allowed, false if denied or unreachable.
fn query_policy_daemon(path: &str, rights: Rights) -> bool {
	use task::scheduler::current_task_id;

	let msg = build_query_msg(path, rights);

	// Acquire the policy daemon's request port.
	let daemon_port = match ipc::IPC_GLOBAL.get_port(0x5000) {
		Some(p) => p,
		None => return true, // default allow on IPC failure
	};

	// Send the query message.
	if daemon_port.send(msg).is_err() {
		return true; // default allow on send failure
	}

	// Try to receive a reply. We use a non-blocking receive on the
	// daemon's reply port. The reply port is determined by the daemon's
	// protocol: it sends replies back to a port derived from the caller's
	// task ID. For simplicity, we poll once and default to allow if
	// no response arrives within one timer tick (~1.6ms).
	let reply_port = 0x5001 + (current_task_id() & 0xFF);
	if let Some(reply_port_obj) = ipc::IPC_GLOBAL.get_port(reply_port) {
		if let Some(reply) = reply_port_obj.receive() {
			return reply.data[0] == 1;
		}
	}

	// Default allow if daemon unreachable.
	true
}

/* ------------------------------------------------------------------ */
/*  Evaluate: canonical LES path                                       */
/* ------------------------------------------------------------------ */

/// Evaluate a path + rights combination against the LES policy.
///
/// Algorithm:
/// 1. If local policy is loaded, check it first.
///    - Deny rules block immediately.
///    - Allow rules require the specified rights.
///    - No match -> default allow.
/// 2. If no local rule matches (or no local policy), query the daemon.
/// 3. Default allow if daemon is unreachable.
///
/// This implements the "least-elevated" principle: only explicitly denied
/// paths are blocked; everything else defaults to allow.
pub fn evaluate(path: &str, rights: Rights) -> bool {
	// Fast path: check local policy.
	if let Some(policy) = get_local_policy() {
		if policy.rule_count() > 0 {
			return policy.allows(path, rights);
		}
	}

	// Slow path: query policy daemon.
	query_policy_daemon(path, rights)
}

/* ------------------------------------------------------------------ */
/*  Initialize with default deny rules                                   */
/* ------------------------------------------------------------------ */

/// Set up the default policy at boot.
///
/// Denies access to sensitive paths:
///   - /etc/shadow (password hashes)
///   - /proc/serix/cap-audit (audit log — admin only)
///
/// Allows everything else.
pub fn init_default_policy() {
	let rules = vec![
		PolicyRule::deny("/etc/shadow"),
		PolicyRule::deny("/proc/serix/cap-audit"),
	];
	set_policy(rules);
}

/* ------------------------------------------------------------------ */
/*  Unit tests                                                         */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn evaluate_no_policy_defaults_allow() {
		// No policy loaded -> default allow.
		assert!(evaluate("/any/path", Rights::READ));
	}

	#[test]
	fn evaluate_load_policy_blocks_deny() {
		let rules = vec![PolicyRule::deny("/etc/shadow")];
		set_policy(rules);

		assert!(!evaluate("/etc/shadow", Rights::READ));

		// Clean up: clear policy.
		set_policy(Vec::new());
	}

	#[test]
	fn evaluate_load_policy_allows_default() {
		let rules = vec![PolicyRule::deny("/etc/shadow")];
		set_policy(rules);

		assert!(evaluate("/etc/passwd", Rights::READ));

		// Clean up.
		set_policy(Vec::new());
	}

	#[test]
	fn evaluate_atomic_swap() {
		let rules1 = vec![PolicyRule::deny("/old/path")];
		set_policy(rules1);

		let rules2 = vec![PolicyRule::deny("/new/path")];
		set_policy(rules2);

		assert!(!evaluate("/new/path", Rights::READ));
		assert!(evaluate("/old/path", Rights::READ)); // old rule cleared

		set_policy(Vec::new());
	}
}
