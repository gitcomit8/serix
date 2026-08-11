/*
 * CapabilitySpace — per-task capability slot table
 *
 * Maps slot indices to (CapabilityId, generation) pairs. Each slot has
 * an optional close-on-exec flag. Generation counters detect stale handles
 * when slots are reused.
 */

use crate::types::CapabilityId;
use alloc::vec::Vec;
use core::cell::Cell;

/// Maximum number of capability slots per task.
/// Large enough for typical workloads; grows dynamically beyond this.
const INITIAL_CAPACITY: usize = 64;

/// A single slot in the capability space.
#[derive(Clone, Debug)]
struct Slot {
	/// Internal capability ID (None = empty slot)
	cap_id: Option<CapabilityId>,
	/// Generation — incremented on each insert, checked on validate
	generation: u32,
	/// Close this slot on execve
	close_on_exec: bool,
}

impl Slot {
	const fn empty() -> Self {
		Slot {
			cap_id: None,
			generation: 0,
			close_on_exec: false,
		}
	}
}

/* ------------------------------------------------------------------ */
/*  CapabilitySpace — per-task cspace                                   */
/* ------------------------------------------------------------------ */

/// Per-task capability space. Manages slot allocation, generation
/// counters, and close-on-exec cleanup.
///
/// Thread-safe via interior mutability (Cell) for single-threaded task
/// context, or wrap in an external Mutex for cross-thread access.
#[derive(Clone, Debug)]
pub struct CapabilitySpace {
	/// Slot table
	slots: Vec<Slot>,
	/// Next generation counter (monotonically increasing)
	next_generation: Cell<u32>,
}

impl CapabilitySpace {
	/*
	 * new - Create an empty capability space
	 */
	pub fn new() -> Self {
		CapabilitySpace {
			slots: Vec::with_capacity(INITIAL_CAPACITY),
			next_generation: Cell::new(1), // 0 = uninitialized
		}
	}

	/*
	 * insert - Place a capability into the next available slot
	 *
	 * Returns the slot index, or None if the capability should not be
	 * placed (e.g., delegation depth exceeded).
	 */
	pub fn insert(
		&mut self,
		cap_id: CapabilityId,
		close_on_exec: bool,
	) -> Option<usize> {
		// Find first empty slot
		let slots = &mut self.slots;
		for (i, slot) in slots.iter_mut().enumerate() {
			if slot.cap_id.is_none() {
				let generation = self.next_generation.get();
				slot.cap_id = Some(cap_id);
				slot.generation = generation;
				slot.close_on_exec = close_on_exec;
				self.next_generation.set(generation + 1);
				return Some(i);
			}
		}

		// No empty slot — grow
		let slot_idx = slots.len();
		slots.push(Slot {
			cap_id: Some(cap_id),
			generation: self.next_generation.get(),
			close_on_exec,
		});
		self.next_generation.set(self.next_generation.get() + 1);
		Some(slot_idx)
	}

	/*
	 * get - Look up a capability by slot index
	 *
	 * Returns (CapabilityId, generation) if the slot is occupied,
	 * None if empty. Does NOT check validity (revocation/expiry) —
	 * that's validate()'s job.
	 */
	pub fn get(&self, slot: usize) -> Option<(CapabilityId, u32)> {
		self.slots.get(slot).and_then(|s| {
			s.cap_id.map(|id| (id, s.generation))
		})
	}

	/*
	 * remove - Remove a capability from a slot
	 */
	pub fn remove(&mut self, slot: usize) {
		if let Some(s) = self.slots.get_mut(slot) {
			s.cap_id = None;
			/* A closed slot must never validate after it is reused. */
			s.generation = self.next_generation.get();
			self.next_generation.set(s.generation.wrapping_add(1).max(1));
			s.close_on_exec = false;
		}
	}

	/*
	 * duplicate - Copy a capability into a new slot (same authority)
	 *
	 * Returns the new slot index.
	 */
	pub fn duplicate(&mut self, source_slot: usize, close_on_exec: bool) -> Option<usize> {
		let (cap_id, _) = self.get(source_slot)?;
		self.insert(cap_id, close_on_exec)
	}

	/*
	 * close_on_exec — Close all slots marked close_on_exec
	 *
	 * Called during execve. Removes the slot and clears the flag.
	 */
	pub fn close_on_exec(&mut self) {
		for slot in 0..self.slots.len() {
			if self.slots[slot].close_on_exec {
				self.remove(slot);
			}
		}
	}

	/*
	 * iter — Iterate over all occupied slots
	 *
	 * Yields (slot_index, CapabilityId, generation).
	 */
	pub fn iter(&self) -> impl Iterator<Item = (usize, CapabilityId, u32)> + '_ {
		self.slots.iter().enumerate().filter_map(|(i, s)| {
			s.cap_id.map(|id| (i, id, s.generation))
		})
	}

	/*
	 * len — Number of slots (including empty)
	 */
	pub fn len(&self) -> usize {
		self.slots.len()
	}

	/*
	 * is_empty — True if no slots allocated
	 */
	pub fn is_empty(&self) -> bool {
		self.slots.is_empty()
	}
}
