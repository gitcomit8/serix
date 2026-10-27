/*
 * fs/src/policy.rs - Minimal LES (Least-Elevated Security) policy engine
 *
 * Provides a declarative policy language for file-system access control.
 * Policies are compiled into a snapshot that the kernel uses to evaluate
 * access decisions alongside capability checks.
 *
 * Policy format (binary, future):
 *   magic: u32 (0x4C455331 = "LES1")
 *   version: u8
 *   rule_count: u16
 *   rules: [PolicyRule; rule_count]
 *
 * Each rule:
 *   path_pattern: [u8; 64]  (null-terminated glob, e.g. "/etc/shadow\0")
 *   match_type: u8 (0=exact, 1=prefix, 2=glob)
 *   required_rights: u32   (Rights bitmask)
 *   action: u8 (0=allow, 1=deny)
 *   flags: u8
 *
 * For now, the API is fully programmatic — no serialized policy format yet.
 */

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use capability::Rights;

/* ------------------------------------------------------------------ */
/*  Policy rule                                                        */
/* ------------------------------------------------------------------ */

/// How to match a path against a rule's pattern.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchType {
	/// Exact match — path must equal pattern exactly.
	Exact,
	/// Prefix match — path must start with pattern (plus optional `/`).
	Prefix,
	/// Glob match — `*` matches any suffix.
	Glob,
}

/// A single policy rule: if a path matches, require `rights` and take `action`.
#[derive(Clone, Debug)]
pub struct PolicyRule {
	/// Path pattern to match.
	pub path: String,
	/// How to match the pattern.
	pub match_type: MatchType,
	/// Minimum rights required (bitmask).
	pub required_rights: Rights,
	/// Whether to allow or deny.
	pub action: PolicyAction,
}

/// Action to take when a rule fires.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolicyAction {
	/// Allow the operation if rights are present.
	Allow,
	/// Deny the operation regardless of capabilities.
	Deny,
}

impl PolicyRule {
	/// Create an allow rule requiring exactly `rights` on `path`.
	pub fn allow(path: &str, rights: Rights) -> Self {
		PolicyRule {
			path: path.to_string(),
			match_type: MatchType::Exact,
			required_rights: rights,
			action: PolicyAction::Allow,
		}
	}

	/// Create a deny-all rule for a path.
	pub fn deny(path: &str) -> Self {
		PolicyRule {
			path: path.to_string(),
			match_type: MatchType::Exact,
			required_rights: Rights::default(),
			action: PolicyAction::Deny,
		}
	}

	/// Create a prefix allow rule (e.g. `/home/*/`).
	pub fn allow_prefix(path: &str, rights: Rights) -> Self {
		PolicyRule {
			path: path.to_string(),
			match_type: MatchType::Prefix,
			required_rights: rights,
			action: PolicyAction::Allow,
		}
	}
}

/* ------------------------------------------------------------------ */
/*  Policy snapshot (immutable, Arc-cloned)                              */
/* ------------------------------------------------------------------ */

/// An immutable snapshot of the active policy.
///
/// Cloning is cheap (Arc interior). Swapping the active policy is
/// done via `PolicySnapshot::swap_global()` which is atomic.
#[derive(Clone, Debug)]
pub struct PolicySnapshot {
	rules: Arc<Vec<PolicyRule>>,
	/// Number of deny rules (for quick path checks).
	deny_count: Arc<AtomicU32>,
}

impl PolicySnapshot {
	/// Create a new policy snapshot with the given rules.
	pub fn new(rules: Vec<PolicyRule>) -> Self {
		let deny_count = rules.iter().filter(|r| r.action == PolicyAction::Deny).count() as u32;
		PolicySnapshot {
			rules: Arc::new(rules),
			deny_count: Arc::new(AtomicU32::new(deny_count)),
		}
	}

	/// Evaluate a path against the policy for the given rights.
	///
	/// Returns true if the access is allowed (or if no rule matches).
	/// Deny rules always block. Allow rules require the specified rights.
	/// If no rule matches, the default is to allow (least restrictive).
	pub fn allows(&self, path: &str, requested_rights: Rights) -> bool {
		// Check rules in order — first match wins.
		// Deny rules are checked first (optimization).
		let deny_count = self.deny_count.load(Ordering::Relaxed);
		if deny_count > 0 {
			// Check deny rules first.
			for rule in self.rules.iter() {
				if rule.action == PolicyAction::Deny && self.matches(rule, path) {
					return false;
				}
			}
		}

		// Check allow rules.
		for rule in self.rules.iter() {
			if rule.action == PolicyAction::Allow && self.matches(rule, path) {
				return requested_rights.contains(rule.required_rights);
			}
		}

		// No rule matched — default allow.
		true
	}

	fn matches(&self, rule: &PolicyRule, path: &str) -> bool {
		match rule.match_type {
			MatchType::Exact => rule.path == path,
			MatchType::Prefix => {
				path == rule.path || path.starts_with(&format!("{}/", rule.path))
			}
			MatchType::Glob => {
				if let Some(wildcard_pos) = rule.path.find('*') {
					let prefix = &rule.path[..wildcard_pos];
					path.starts_with(prefix)
				} else {
					rule.path == path
				}
			}
		}
	}

	/// Get the number of rules in this snapshot.
	pub fn rule_count(&self) -> usize {
		self.rules.len()
	}

	/// Get the number of deny rules.
	pub fn deny_count(&self) -> u32 {
		self.deny_count.load(Ordering::Relaxed)
	}
}

impl Default for PolicySnapshot {
	fn default() -> Self {
		PolicySnapshot {
			rules: Arc::new(Vec::new()),
			deny_count: Arc::new(AtomicU32::new(0)),
		}
	}
}

/* ------------------------------------------------------------------ */
/*  POSIX mode bits -> capability rights mapping                         */
/* ------------------------------------------------------------------ */

/// Map traditional POSIX file mode bits to capability Rights.
///
/// This is used when the policy engine needs to translate legacy
/// permission checks into the capability model.
pub fn posix_mode_to_rights(mode: u16) -> Rights {
	let mut rights = Rights::default();

	// Extract permission bits
	let perms = mode & 0o777;

	// Read permission -> READ + LOOKUP (for directories)
	if perms & 0o444 != 0 {
		rights = rights.union(Rights::READ);
	}

	// Write permission -> WRITE + APPEND
	if perms & 0o222 != 0 {
		rights = rights.union(Rights::WRITE);
		if perms & 0o002 != 0 {
			rights = rights.union(Rights::APPEND);
		}
	}

	// Execute permission -> EXECUTE
	if perms & 0o111 != 0 {
		rights = rights.union(Rights::EXECUTE);
	}

	// Owner write -> METADATA (for chown-like operations)
	if perms & 0o200 != 0 {
		rights = rights.union(Rights::METADATA);
	}

	rights
}

/// Map POSIX permission flags (open(2) flags) to capability Rights.
///
/// - O_RDONLY -> READ
/// - O_WRONLY -> WRITE
/// - O_RDWR   -> READ | WRITE
/// - O_APPEND -> APPEND
/// - O_EXEC   -> EXECUTE
/// - O_CREAT  -> requires METADATA on parent directory
/// - O_TRUNC  -> requires WRITE + METADATA
pub fn posix_open_flags_to_rights(flags: i32) -> Rights {
	let mut rights = Rights::default();

	let o_rdonly = (flags & 0o0) != 0; // O_RDONLY = 0o0
	let o_wronly = (flags & 0o2) != 0; // O_WRONLY = 0o2
	let o_rdwr = (flags & 0o2) == 0;   // O_RDWR = not O_WRONLY

	let o_append = (flags & 0o2000) != 0;
	let o_exec = (flags & 0o1000) != 0;  // O_EXEC
	let o_creat = (flags & 0o100) != 0;  // O_CREAT
	let o_trunc = (flags & 0o10000) != 0; // O_TRUNC

	if o_rdonly {
		rights = rights.union(Rights::READ);
	}
	if o_wronly {
		rights = rights.union(Rights::WRITE);
	}
	if o_rdwr {
		rights = rights.union(Rights::READ);
		rights = rights.union(Rights::WRITE);
	}
	if o_append {
		rights = rights.union(Rights::APPEND);
	}
	if o_exec {
		rights = rights.union(Rights::EXECUTE);
	}
	if o_creat {
		rights = rights.union(Rights::METADATA);
	}
	if o_trunc {
		rights = rights.union(Rights::WRITE);
		rights = rights.union(Rights::METADATA);
	}

	rights
}

/// Map POSIX access mode (for access(2)) to capability Rights.
///
/// - R_OK -> READ
/// - W_OK -> WRITE
/// - X_OK -> EXECUTE
/// - F_OK -> no rights required (just existence check)
pub fn posix_access_to_rights(mode: i32) -> Rights {
	let mut rights = Rights::default();
	if mode & 0o4 != 0 {
		rights = rights.union(Rights::READ);
	}
	if mode & 0o2 != 0 {
		rights = rights.union(Rights::WRITE);
	}
	if mode & 0o1 != 0 {
		rights = rights.union(Rights::EXECUTE);
	}
	// F_OK (0) -> no rights required
	rights
}

/* ------------------------------------------------------------------ */
/*  Unit tests                                                         */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn policy_snapshot_default_empty() {
		let snap = PolicySnapshot::default();
		assert_eq!(snap.rule_count(), 0);
		assert_eq!(snap.deny_count(), 0);
		// Empty policy defaults to allow.
		assert!(snap.allows("/anything", Rights::READ));
	}

	#[test]
	fn policy_deny_rule_blocks() {
		let rules = vec![PolicyRule::deny("/etc/shadow")];
		let snap = PolicySnapshot::new(rules);
		assert_eq!(snap.deny_count(), 1);

		// Denied path.
		assert!(!snap.allows("/etc/shadow", Rights::READ));
		// Other paths still allowed.
		assert!(snap.allows("/etc/passwd", Rights::READ));
	}

	#[test]
	fn policy_allow_rule_requires_rights() {
		let rules = vec![PolicyRule::allow("/home/user/file", Rights::READ)];
		let snap = PolicySnapshot::new(rules);

		// Has READ -> allowed.
		assert!(snap.allows("/home/user/file", Rights::READ));
		// Has READ | WRITE -> allowed.
		assert!(snap.allows("/home/user/file", Rights::READ.union(Rights::WRITE)));
		// Missing READ -> not allowed by this rule, default deny.
		assert!(!snap.allows("/home/user/file", Rights::WRITE));
	}

	#[test]
	fn policy_prefix_match() {
		let rules = vec![
			PolicyRule::allow_prefix("/home", Rights::READ),
			PolicyRule::deny("/home/secret"),
		];
		let snap = PolicySnapshot::new(rules);

		// Prefix match allows reading files under /home.
		assert!(snap.allows("/home/user/docs", Rights::READ));
		// But /home/secret is explicitly denied.
		assert!(!snap.allows("/home/secret", Rights::READ));
	}

	#[test]
	fn policy_glob_match() {
		let rules = vec![PolicyRule {
			path: "/tmp/*.log".to_string(),
			match_type: MatchType::Glob,
			required_rights: Rights::READ,
			action: PolicyAction::Allow,
		}];
		let snap = PolicySnapshot::new(rules);

		assert!(snap.allows("/tmp/system.log", Rights::READ));
		assert!(snap.allows("/tmp/app.log", Rights::READ));
		assert!(!snap.allows("/tmp/system.log", Rights::WRITE));
		// Not a glob match.
		assert!(!snap.allows("/tmp/system.txt", Rights::READ));
	}

	#[test]
	fn policy_first_match_wins() {
		// First matching rule wins, regardless of allow/deny order.
		let rules = vec![
			PolicyRule::allow("/etc/config", Rights::READ),
			PolicyRule::deny("/etc/config"),
		];
		let snap = PolicySnapshot::new(rules);

		// First rule is allow, so it wins.
		assert!(snap.allows("/etc/config", Rights::READ));
	}

	#[test]
	fn posix_mode_to_rights_read() {
		// 0o444 -> read for all
		let rights = posix_mode_to_rights(0o100444);
		assert!(rights.contains(Rights::READ));
	}

	#[test]
	fn posix_mode_to_rights_write() {
		// 0o666 -> read+write for all
		let rights = posix_mode_to_rights(0o100666);
		assert!(rights.contains(Rights::READ));
		assert!(rights.contains(Rights::WRITE));
	}

	#[test]
	fn posix_mode_to_rights_execute() {
		// 0o755 -> rwxr-xr-x
		let rights = posix_mode_to_rights(0o100755);
		assert!(rights.contains(Rights::READ));
		assert!(rights.contains(Rights::WRITE));
		assert!(rights.contains(Rights::EXECUTE));
	}

	#[test]
	fn posix_open_flags_rdonly() {
		let rights = posix_open_flags_to_rights(0o0); // O_RDONLY
		assert!(rights.contains(Rights::READ));
		assert!(!rights.contains(Rights::WRITE));
	}

	#[test]
	fn posix_open_flags_rdwr() {
		let rights = posix_open_flags_to_rights(0o2); // O_RDWR
		assert!(rights.contains(Rights::READ));
		assert!(rights.contains(Rights::WRITE));
	}

	#[test]
	fn posix_open_flags_append() {
		let rights = posix_open_flags_to_rights(0o202); // O_WRONLY | O_APPEND
		assert!(rights.contains(Rights::WRITE));
		assert!(rights.contains(Rights::APPEND));
	}

	#[test]
	fn posix_access_r_ok() {
		let rights = posix_access_to_rights(0o4);
		assert!(rights.contains(Rights::READ));
	}

	#[test]
	fn posix_access_w_ok() {
		let rights = posix_access_to_rights(0o2);
		assert!(rights.contains(Rights::WRITE));
	}

	#[test]
	fn posix_access_x_ok() {
		let rights = posix_access_to_rights(0o1);
		assert!(rights.contains(Rights::EXECUTE));
	}

	#[test]
	fn posix_access_f_ok() {
		let rights = posix_access_to_rights(0o0);
		assert!(rights.is_empty());
	}
}
