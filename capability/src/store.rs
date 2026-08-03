/*
 * CapabilityStore — authoritative capability registry
 *
 * Maps CapabilityId → CapabilityRecord. Provides lifecycle operations:
 * mint, insert, duplicate, delegate, remove, revoke, expire.
 * Thread-safe via Mutex.
 */

use crate::types::{
	CapabilityHandle, CapabilityId, CapabilityRecord, CapabilityType, Rights,
};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/* ------------------------------------------------------------------ */
/*  CapabilityStore — authoritative store                              */
/* ------------------------------------------------------------------ */

pub struct CapabilityStore {
	/// Authoritative record map: CapabilityId → CapabilityRecord
	records: Mutex<BTreeMap<CapabilityId, CapabilityRecord>>,
	/// Next CapabilityId to assign (monotonically increasing)
	next_id: Mutex<u64>,
	/// Reverse index: handle → CapabilityId (for handle-based lookup)
	handle_index: Mutex<BTreeMap<[u8; 16], CapabilityId>>,
}

impl CapabilityStore {
	/*
	 * new - Create an empty capability store
	 */
	pub const fn new() -> Self {
		CapabilityStore {
			records: Mutex::new(BTreeMap::new()),
			next_id: Mutex::new(1), // 0 is reserved
			handle_index: Mutex::new(BTreeMap::new()),
		}
	}

	/*
	 * mint_root - Create a root capability (no parent)
	 *
	 * Called during bootstrap for PID 1 / Server Manager.
	 * Returns the CapabilityRecord (caller inserts into cspace separately).
	 */
	pub fn mint_root(
		&self,
		handle: CapabilityHandle,
		cap_type: CapabilityType,
		rights: Rights,
		expiry_ticks: u64,
	) -> CapabilityRecord {
		let mut id_lock = self.next_id.lock();
		let id = CapabilityId::new(*id_lock);
		*id_lock += 1;
		drop(id_lock);

		let record = CapabilityRecord {
			id,
			handle,
			cap_type,
			rights,
			parent: None,
			generation: 1,
			expiry_ticks,
			revoked: false,
			close_on_exec: false,
		};

		let mut records = self.records.lock();
		records.insert(id, record.clone());

		let mut handle_idx = self.handle_index.lock();
		handle_idx.insert(handle.key, id);

		record
	}

	/*
	 * insert_into_space - Insert an existing record into the store
	 *
	 * Used when a capability was created externally (e.g., by ipc::create_port)
	 * and needs to be registered in the store.
	 */
	pub fn insert_into_space(&self, record: CapabilityRecord) -> CapabilityId {
		let mut records = self.records.lock();
		records.insert(record.id, record.clone());
		drop(records);

		let mut handle_idx = self.handle_index.lock();
		handle_idx.insert(record.handle.key, record.id);

		record.id
	}

	/*
	 * lookup — Find a record by CapabilityId
	 */
	pub fn lookup(&self, id: CapabilityId) -> Option<CapabilityRecord> {
		self.records.lock().get(&id).cloned()
	}

	/*
	 * lookup_by_handle — Find a record by 128-bit handle key
	 */
	pub fn lookup_by_handle(&self, key: &[u8; 16]) -> Option<CapabilityRecord> {
		let handle_idx = self.handle_index.lock();
		let id = handle_idx.get(key)?;
		self.records.lock().get(id).cloned()
	}

	/*
	 * duplicate — Create a duplicate of a capability (same authority)
	 *
	 * Returns a new CapabilityRecord with a new handle and CapabilityId,
	 * but the same cap_type, rights, and object scope.
	 */
	pub fn duplicate(&self, source: &CapabilityRecord) -> CapabilityRecord {
		let mut id_lock = self.next_id.lock();
		let id = CapabilityId::new(*id_lock);
		*id_lock += 1;
		drop(id_lock);

		let handle = CapabilityHandle::generate();

		let record = CapabilityRecord {
			id,
			handle,
			cap_type: source.cap_type,
			rights: source.rights,
			parent: Some(source.id),
			generation: 1,
			expiry_ticks: source.expiry_ticks, // inherits parent expiry
			revoked: false,
			close_on_exec: source.close_on_exec,
		};

		let mut records = self.records.lock();
		records.insert(id, record.clone());

		let mut handle_idx = self.handle_index.lock();
		handle_idx.insert(handle.key, id);

		record
	}

	/*
	 * delegate — Create a restricted child capability
	 *
	 * @parent: The parent capability record
	 * @reduced_rights: Rights subset of parent's rights
	 * @expiry_ticks: Must be <= parent's expiry_ticks (0 = inherit)
	 *
	 * Returns a new CapabilityRecord. Caller inserts into child's cspace.
	 */
	pub fn delegate(
		&self,
		parent: &CapabilityRecord,
		reduced_rights: Rights,
		expiry_ticks: u64,
	) -> Result<CapabilityRecord, DelegateError> {
		// Rights must be a strict subset
		if !parent.rights.contains(reduced_rights) {
			return Err(DelegateError::RightsSuperset);
		}

		// Expiry must not exceed parent's
		if expiry_ticks != 0 && parent.expiry_ticks != 0 && expiry_ticks > parent.expiry_ticks {
			return Err(DelegateError::ExpiryExceedsParent);
		}

		let mut id_lock = self.next_id.lock();
		let id = CapabilityId::new(*id_lock);
		*id_lock += 1;
		drop(id_lock);

		let handle = CapabilityHandle::generate();

		let effective_expiry = if expiry_ticks != 0 {
			expiry_ticks
		} else if parent.expiry_ticks != 0 {
			parent.expiry_ticks
		} else {
			0 // no expiry
		};

		let record = CapabilityRecord {
			id,
			handle,
			cap_type: parent.cap_type,
			rights: reduced_rights,
			parent: Some(parent.id),
			generation: 1,
			expiry_ticks: effective_expiry,
			revoked: false,
			close_on_exec: false,
		};

		let mut records = self.records.lock();
		records.insert(id, record.clone());

		let mut handle_idx = self.handle_index.lock();
		handle_idx.insert(handle.key, id);

		Ok(record)
	}

	/*
	 * remove — Remove a capability from the store
	 */
	pub fn remove(&self, id: CapabilityId) {
		let record = self.records.lock().get(&id).cloned();
		if let Some(record) = record {
			self.records.lock().remove(&id);
			let mut handle_idx = self.handle_index.lock();
			handle_idx.remove(&record.handle.key);
		}
	}

	/*
	 * revoke — Revoke a capability and all its descendants
	 *
	 * Marks the record and all descendants as revoked.
	 * Synchronous: after this returns, no CPU can validate the capability.
	 */
	pub fn revoke(&self, id: CapabilityId) {
		let mut records = self.records.lock();

		// Mark the target
		if let Some(rec) = records.get_mut(&id) {
			rec.revoked = true;
		}

		// Mark all descendants (simple traversal; optimize with epoch tree later)
		let descendant_ids: Vec<CapabilityId> = records
			.values()
			.filter(|r| r.parent == Some(id) && !r.revoked)
			.map(|r| r.id)
			.collect();

		for desc_id in descendant_ids {
			if let Some(rec) = records.get_mut(&desc_id) {
				rec.revoked = true;
			}
		}
	}

	/*
	 * get_descendants — List all direct child CapabilityIds
	 */
	pub fn get_descendants(&self, parent_id: CapabilityId) -> Vec<CapabilityId> {
		self.records
			.lock()
			.values()
			.filter(|r| r.parent == Some(parent_id))
			.map(|r| r.id)
			.collect()
	}

	/*
	 * count — Number of records in the store
	 */
	pub fn count(&self) -> usize {
		self.records.lock().len()
	}
}

/* ------------------------------------------------------------------ */
/*  DelegateError — why delegation failed                              */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelegateError {
	/// Requested rights are not a subset of parent's rights
	RightsSuperset,
	/// Requested expiry exceeds parent's expiry
	ExpiryExceedsParent,
}
