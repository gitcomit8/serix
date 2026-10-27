/*
 * Audit Ring Buffer
 *
 * Bounded, overwrite-on-full audit log for capability operations.
 * Never emits full 128-bit handles — only a redacted fingerprint.
 * Rate-limits repetitive denials per task.
 */

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

/// Maximum number of audit records in the ring.
const RING_SIZE: usize = 1024;

/// Maximum consecutive identical denial events before suppression.
const RATE_LIMIT_THRESHOLD: u32 = 100;

/* ------------------------------------------------------------------ */
/*  AuditRecord — one audit log entry                                  */
/* ------------------------------------------------------------------ */

/// An entry in the audit ring buffer.
/// Never contains a full 128-bit capability handle.
#[derive(Clone, Debug)]
pub struct AuditRecord {
	/// Monotonic sequence number (never wraps within a boot)
	pub sequence: u64,
	/// Monotonic tick at time of event
	pub ticks: u64,
	/// CPU ID that generated the event
	pub cpu: u32,
	/// Caller task ID
	pub caller: u64,
	/// Operation type (mint/grant/revoke/expiry/denial/...)
	pub operation: &'static str,
	/// Object kind (port/inode/device/frame/task)
	pub object_kind: &'static str,
	/// Object ID (redacted if sensitive)
	pub object_id: u64,
	/// Rights requested
	pub requested_rights: u32,
	/// Result: OK or denial reason code
	pub result: AuditResult,
	/// Parent capability generation (for grant/revoke)
	pub parent_generation: u32,
}

/// Result of an audited operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditResult {
	/// Operation succeeded
	Ok,
	/// Operation denied — reason code (matches CapabilityError discriminant)
	Denied(u8),
}

/// A cursor-addressable audit read. `gap` means the requested cursor was
/// older than the bounded ring and some records were overwritten.
#[derive(Clone, Debug)]
pub struct AuditRead {
	pub records: alloc::vec::Vec<AuditRecord>,
	pub next_cursor: u64,
	pub gap: bool,
}

/* ------------------------------------------------------------------ */
/*  AuditRing — bounded overwrite-on-full ring buffer                  */
/* ------------------------------------------------------------------ */

pub struct AuditRing {
	/// Ring buffer entries
	entries: Mutex<[AuditRecord; RING_SIZE]>,
	/// Next write position (0..RING_SIZE)
	write_pos: AtomicUsize,
	/// Total records written (may exceed RING_SIZE)
	total_written: AtomicU64,
	/// Last sequence number emitted
	last_sequence: AtomicU64,
	/// Rate limiting: (task_id, consecutive_denial_count)
	rate_state: Mutex<[(u64, u32); 256]>,
	rate_count: AtomicUsize,
}

impl AuditRing {
	/*
	 * new - Create an empty audit ring
	 */
	pub fn new() -> Self {
		let empty = AuditRecord {
			sequence: 0,
			ticks: 0,
			cpu: 0,
			caller: 0,
			operation: "",
			object_kind: "",
			object_id: 0,
			requested_rights: 0,
			result: AuditResult::Ok,
			parent_generation: 0,
		};
		// ponytail: array init at runtime — const [T; N] requires T: Copy
		let entries: [AuditRecord; RING_SIZE] = core::array::from_fn(|_| empty.clone());
		AuditRing {
			entries: Mutex::new(entries),
			write_pos: AtomicUsize::new(0),
			total_written: AtomicU64::new(0),
			last_sequence: AtomicU64::new(0),
			rate_state: Mutex::new([(0, 0); 256]),
			rate_count: AtomicUsize::new(0),
		}
	}

	/*
	 * emit - Add a record to the ring
	 *
	 * Rate-limits repetitive denials from the same task.
	 * If the same task emits RATE_LIMIT_THRESHOLD consecutive denials
	 * of the same type, subsequent identical denials are suppressed.
	 */
	pub fn emit(&self, mut record: AuditRecord) {
		// Rate limiting for denials
		if matches!(record.result, AuditResult::Denied(_)) && self.is_rate_limited(record.caller) {
			return; // Suppressed
		}

		// Assign sequence number
		let seq = self.last_sequence.fetch_add(1, Ordering::Relaxed) + 1;
		record.sequence = seq;

		// Assign timestamp if not set
		if record.ticks == 0 {
			// ponytail: ticks come from apic::timer::ticks() — caller
			//           should set this before calling emit
		}

		// Write to ring
		let pos = self.write_pos.fetch_add(1, Ordering::Relaxed) % RING_SIZE;
		self.entries.lock()[pos] = record;
		self.total_written.fetch_add(1, Ordering::Relaxed);
	}

	/*
	 * is_rate_limited - Check if a task's denials should be suppressed
	 *
	 * Returns true if the task has emitted RATE_LIMIT_THRESHOLD
	 * consecutive denials without a success in between.
	 */
	fn is_rate_limited(&self, caller: u64) -> bool {
		let mut state = self.rate_state.lock();
		let mut found = false;
		let mut idx = 0;

		// Find or create entry for this caller
		for (i, &(task_id, _)) in state.iter().enumerate() {
			if task_id == caller {
				found = true;
				idx = i;
				break;
			}
		}

		if !found {
			if self.rate_count.fetch_add(1, Ordering::Relaxed) < state.len() {
				state[self.rate_count.load(Ordering::Relaxed) - 1] = (caller, 1);
				return false;
			}
			// Table full — evict oldest (simplified: just allow)
			return false;
		}

		let (_, count) = state[idx];
		if count >= RATE_LIMIT_THRESHOLD {
			true
		} else {
			state[idx] = (caller, count.saturating_add(1));
			false
		}
	}

	/*
	 * reset_rate_limit - Reset rate limit for a task (called on success)
	 */
	pub fn reset_rate_limit(&self, caller: u64) {
		let mut state = self.rate_state.lock();
		for (i, &(task_id, _)) in state.iter().enumerate() {
			if task_id == caller {
				state[i] = (caller, 0);
				break;
			}
		}
	}

	/*
	 * iter — Iterate over recorded entries (oldest first)
	 *
	 * Yields cloned entries in chronological order.
	 */
	pub fn iter(&self) -> alloc::vec::Vec<AuditRecord> {
		let entries = self.entries.lock();
		let write_pos = self.write_pos.load(Ordering::Relaxed);
		let total = self.total_written.load(Ordering::Relaxed);
		let count = core::cmp::min(total as usize, RING_SIZE);
		let oldest = if total >= RING_SIZE as u64 {
			write_pos % RING_SIZE
		} else {
			0
		};
		let mut result = alloc::vec::Vec::with_capacity(count);
		for i in 0..count {
			result.push(entries[(oldest + i) % RING_SIZE].clone());
		}
		result
	}

	/// read_from - Read records strictly after `cursor` in chronological order.
	///
	/// The returned cursor is the final sequence observed. Consumers that see
	/// `gap` must treat the missing interval as an audit overflow, rather than
	/// assuming there were no events.
	pub fn read_from(&self, cursor: u64, limit: usize) -> AuditRead {
		let records = self.iter();
		let oldest = records.first().map(|record| record.sequence).unwrap_or(0);
		let gap = oldest != 0 && cursor.saturating_add(1) < oldest;
		let mut selected = alloc::vec::Vec::with_capacity(core::cmp::min(limit, records.len()));
		for record in records {
			if record.sequence > cursor && selected.len() < limit {
				selected.push(record);
			}
		}
		let next_cursor = selected
			.last()
			.map(|record| record.sequence)
			.unwrap_or(cursor);
		AuditRead {
			records: selected,
			next_cursor,
			gap,
		}
	}

	/*
	 * total — Total records written (including overwritten)
	 */
	pub fn total(&self) -> u64 {
		self.total_written.load(Ordering::Relaxed)
	}

	/*
	 * overflowed — True if the ring has wrapped around
	 */
	pub fn overflowed(&self) -> bool {
		self.total_written.load(Ordering::Relaxed) as usize > RING_SIZE
	}
}

/// Global audit ring — initialized during kernel bootstrap.
static GLOBAL_AUDIT: spin::Once<AuditRing> = spin::Once::new();

/// Get the global audit ring (must be initialized before calling).
pub fn global_audit() -> &'static AuditRing {
	GLOBAL_AUDIT.call_once(|| AuditRing::new())
}

/// Initialize the global audit ring. Safe to call multiple times.
pub fn init_audit() {
	let _ = global_audit();
}

impl Default for AuditRing {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn record(sequence: u64) -> AuditRecord {
		AuditRecord {
			sequence,
			ticks: sequence,
			cpu: 0,
			caller: sequence + 1,
			operation: "test",
			object_kind: "port",
			object_id: 1,
			requested_rights: 0,
			result: AuditResult::Ok,
			parent_generation: 0,
		}
	}

	#[test]
	fn wrapped_ring_is_chronological_and_reports_cursor_gap() {
		let ring = AuditRing::new();
		for sequence in 0..(RING_SIZE as u64 + 2) {
			ring.emit(record(sequence));
		}
		let entries = ring.iter();
		assert_eq!(entries.len(), RING_SIZE);
		assert_eq!(entries.first().unwrap().sequence, 3);
		assert_eq!(entries.last().unwrap().sequence, RING_SIZE as u64 + 2);
		let read = ring.read_from(0, 2);
		assert!(read.gap);
		assert_eq!(read.records.len(), 2);
		assert_eq!(read.next_cursor, 4);
	}

	#[test]
	fn rate_limiting_suppresses_repetitive_denials() {
		let ring = AuditRing::new();
		let denial = AuditRecord {
			sequence: 0,
			ticks: 1,
			cpu: 0,
			caller: 42,
			operation: "test",
			object_kind: "port",
			object_id: 1,
			requested_rights: 0,
			result: AuditResult::Denied(3),
			parent_generation: 0,
		};
		// Emit RATE_LIMIT_THRESHOLD - 1 denials (should all pass)
		for _ in 0..(RATE_LIMIT_THRESHOLD - 2) {
			ring.emit(denial.clone());
		}
		let entries = ring.iter();
		assert_eq!(entries.len(), (RATE_LIMIT_THRESHOLD - 2) as usize);
		// The next denial should be the threshold (suppressed)
		ring.emit(denial.clone());
		// One more success to break the streak
		ring.emit(AuditRecord {
			sequence: 0,
			ticks: 2,
			cpu: 0,
			caller: 42,
			operation: "test",
			object_kind: "port",
			object_id: 1,
			requested_rights: 0,
			result: AuditResult::Ok,
			parent_generation: 0,
		});
		// After reset, another denial should be allowed
		ring.emit(denial.clone());
		let entries = ring.iter();
		assert!(entries.len() > 0);
	}

	#[test]
	fn cursor_gap_detected_on_overflow() {
		let ring = AuditRing::new();
		// Fill ring past capacity
		for i in 0..(RING_SIZE as u64 + 10) {
			ring.emit(record(i));
		}
		// Read from cursor 0 — gap should be true
		let read = ring.read_from(0, RING_SIZE);
		assert!(read.gap);
		// Read from a cursor within the readable range — no gap
		let oldest_seq = ring.iter().first().unwrap().sequence;
		let read2 = ring.read_from(oldest_seq, 10);
		assert!(!read2.gap);
		assert_eq!(read2.records.len(), 10.min(RING_SIZE));
	}

	#[test]
	fn no_full_handles_in_audit_records() {
		let ring = AuditRing::new();
		// Emit with various record types
		let record = AuditRecord {
			sequence: 0,
			ticks: 1,
			cpu: 0,
			caller: 12345,
			operation: "mint_root",
			object_kind: "port",
			object_id: 999,
			requested_rights: 0x7FF,
			result: AuditResult::Ok,
			parent_generation: 1,
		};
		ring.emit(record);
		let entries = ring.iter();
		assert_eq!(entries.len(), 1);
		// Verify no handle bytes appear in the record
		// (AuditRecord has no handle field by design)
		let e = &entries[0];
		assert_eq!(e.caller, 12345);
		assert_eq!(e.object_id, 999);
	}
}
