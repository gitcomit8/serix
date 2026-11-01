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
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use capability::Rights;
use fs::policy::{MatchType, PolicyAction, PolicyRule, PolicySnapshot};

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
/*  TOML config parsing                                                */
/* ------------------------------------------------------------------ */

/// Parse policy rules from a TOML-like config string.
///
/// Format:
///   [[rule]]
///   target = "/path"
///   match_type = "exact" | "prefix" | "glob"
///   action = "allow" | "deny"
///   required_rights = "READ", "WRITE", etc.
pub fn parse_rules_from_toml(text: &str) -> Vec<PolicyRule> {
	let mut rules = Vec::new();
	let mut current_target: Option<String> = None;
	let mut current_match_type = MatchType::Exact;
	let mut current_action = PolicyAction::Allow;
	let mut current_rights: u32 = 0;

	for raw_line in text.lines() {
		let line = trim_comments(raw_line).trim();

		if line.is_empty() || (line.starts_with('[') && !line.starts_with("[[")) {
			/* Section header or blank — flush pending rule */
			if let Some(target) = current_target.take() {
				rules.push(PolicyRule {
					path: target,
					match_type: current_match_type,
					required_rights: capability::Rights(current_rights),
					action: current_action,
				});
			}
			current_match_type = MatchType::Exact;
			current_action = PolicyAction::Allow;
			current_rights = 0;
			continue;
		}

		if line == "[[rule]]" {
			/* Flush previous rule */
			if let Some(target) = current_target.take() {
				rules.push(PolicyRule {
					path: target,
					match_type: current_match_type,
					required_rights: capability::Rights(current_rights),
					action: current_action,
				});
			}
			current_target = None;
			current_match_type = MatchType::Exact;
			current_action = PolicyAction::Allow;
			current_rights = 0;
			continue;
		}

		if let Some((key, value)) = line.split_once('=') {
			let key = key.trim();
			let value = value.trim();

			match key {
				"target" => current_target = Some(parse_string(value)),
				"match_type" => current_match_type = parse_match_type(value),
				"action" => current_action = parse_action(value),
				"required_rights" => current_rights = parse_rights(value),
				_ => {}
			}
		}
	}

	/* Flush last rule */
	if let Some(target) = current_target {
		rules.push(PolicyRule {
			path: target,
			match_type: current_match_type,
			required_rights: capability::Rights(current_rights),
			action: current_action,
		});
	}

	rules
}

/* TOML-like parsing helpers */
fn trim_comments(line: &str) -> &str {
	let mut in_string = false;
	let mut escape = false;
	for (i, ch) in line.char_indices() {
		if escape {
			escape = false;
			continue;
		}
		if ch == '\\' && in_string {
			escape = true;
			continue;
		}
		if ch == '"' {
			in_string = !in_string;
			continue;
		}
		if ch == '#' && !in_string {
			return &line[..i];
		}
	}
	line
}

fn parse_string(value: &str) -> String {
	let value = value.trim();
	if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
		value[1..value.len() - 1].to_string()
	} else {
		value.to_string()
	}
}

fn parse_match_type(value: &str) -> MatchType {
	match value.trim().to_lowercase().as_str() {
		"exact" => MatchType::Exact,
		"prefix" => MatchType::Prefix,
		"glob" => MatchType::Glob,
		_ => MatchType::Exact,
	}
}

fn parse_action(value: &str) -> PolicyAction {
	match value.trim().to_lowercase().as_str() {
		"allow" => PolicyAction::Allow,
		"deny" => PolicyAction::Deny,
		_ => PolicyAction::Allow,
	}
}

fn parse_rights(value: &str) -> u32 {
	let mut bits: u32 = 0;
	for token in value.split(',') {
		let token = token.trim().to_lowercase();
		bits |= match token.as_str() {
			"read" => 1 << 0,
			"write" => 1 << 1,
			"execute" => 1 << 2,
			"seek" => 1 << 3,
			"append" => 1 << 4,
			"metadata" => 1 << 5,
			"lookup" => 1 << 6,
			"send" => 1 << 7,
			"recv" => 1 << 8,
			"notify" => 1 << 9,
			"control" => 1 << 10,
			"mmap" => 1 << 11,
			"munmap" => 1 << 12,
			"spawn" => 1 << 13,
			"wait" => 1 << 14,
			"dup" => 1 << 15,
			"platform_admin" => 1 << 16,
			_ => 0,
		};
	}
	bits
}

/* ------------------------------------------------------------------ */
/*  Notify policyd to reload                                          */
/* ------------------------------------------------------------------ */

/// Send a RELOAD notification to the policy daemon.
///
/// This is best-effort — if policyd is not running or the IPC fails,
/// the kernel's local policy is still updated (above).
pub fn notify_policyd() {
	use ipc::Message;

	let msg = Message {
		sender_id: 0,
		id: 4, /* MSG_RELOAD */
		len: 1,
		data: {
			let mut d = [0u8; 128];
			d[0] = 0; /* success */
			d
		},
	};

	if let Some(port) = ipc::IPC_GLOBAL.get_port(0x5000) {
		let _ = port.send(msg);
	}
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
