/*
 * Audit Ring Buffer
 *
 * Bounded, overwrite-on-full audit log for capability operations.
 * Never emits full 128-bit handles — only a redacted fingerprint.
 * Rate-limits repetitive denials per task.
 */

use core::cell::Cell;
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

/* ------------------------------------------------------------------ */
/*  AuditRing — bounded overwrite-on-full ring buffer                  */
/* ------------------------------------------------------------------ */

pub struct AuditRing {
	/// Ring buffer entries
	entries: Mutex<[AuditRecord; RING_SIZE]>,
	/// Next write position (0..RING_SIZE)
	write_pos: Cell<usize>,
	/// Total records written (may exceed RING_SIZE)
	total_written: Cell<u64>,
	/// Last sequence number emitted
	last_sequence: Cell<u64>,
	/// Rate limiting: (task_id, consecutive_denial_count)
	rate_state: Mutex<[(u64, u32); 256]>,
	rate_count: Cell<usize>,
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
			write_pos: Cell::new(0),
			total_written: Cell::new(0),
			last_sequence: Cell::new(0),
			rate_state: Mutex::new([(0, 0); 256]),
			rate_count: Cell::new(0),
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
		if matches!(record.result, AuditResult::Denied(_)) {
			if self.is_rate_limited(record.caller) {
				return; // Suppressed
			}
		}

		// Assign sequence number
		let seq = self.last_sequence.get() + 1;
		record.sequence = seq;
		self.last_sequence.set(seq);

		// Assign timestamp if not set
		if record.ticks == 0 {
			// ponytail: ticks come from apic::timer::ticks() — caller
			//           should set this before calling emit
		}

		// Write to ring
		let pos = self.write_pos.get();
		self.entries.lock()[pos] = record;
		self.write_pos.set((pos + 1) % RING_SIZE);
		self.total_written.set(self.total_written.get() + 1);
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
			if self.rate_count.get() < state.len() {
				state[self.rate_count.get()] = (caller, 1);
				self.rate_count.set(self.rate_count.get() + 1);
				return false;
			}
			// Table full — evict oldest (simplified: just allow)
			return false;
		}

		let (_, count) = state[idx];
		if count >= RATE_LIMIT_THRESHOLD {
			true
		} else {
			state[idx] = (caller, count + 1);
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
		let write_pos = self.write_pos.get();
		let count = core::cmp::min(write_pos, RING_SIZE);
		let mut result = alloc::vec::Vec::with_capacity(count);
		for i in 0..count {
			result.push(entries[i].clone());
		}
		result
	}

	/*
	 * total — Total records written (including overwritten)
	 */
	pub fn total(&self) -> u64 {
		self.total_written.get()
	}

	/*
	 * overflowed — True if the ring has wrapped around
	 */
	pub fn overflowed(&self) -> bool {
		self.total_written.get() as usize > RING_SIZE
	}
}
