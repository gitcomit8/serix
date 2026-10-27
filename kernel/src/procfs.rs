/*
 * kernel/src/procfs.rs - Minimal /proc filesystem for capability audit
 *
 * Provides /proc/serix/cap-audit, a read-only file that returns serialized
 * audit records from the global audit ring buffer.
 *
 * Format (written into a RamFile-backed buffer on each read):
 *
 *     struct AuditResponse {
 *         next_cursor: u64;
 *         gap: u8;
 *         count: u16;
 *         reserved: u16;
 *         entries: [AuditEntry; count];
 *     };
 *
 *     struct AuditEntry {
 *         sequence: u64;
 *         ticks: u64;
 *         cpu: u32;
 *         caller: u64;
 *         operation_len: u8;
 *         operation: [u8; 31];
 *         object_kind_len: u8;
 *         object_kind: [u8; 31];
 *         object_id: u64;
 *         requested_rights: u32;
 *         result: u8;
 *         parent_generation: u32;
 *         reserved: u32;
 *     };
 *
 * Total entry size: 96 bytes. Max 64 entries per read.
 * Max response: 16 + 64 * 96 = 6160 bytes.
 */

use alloc::sync::Arc;
use alloc::vec::Vec;

/// Build a RamFile containing serialized audit records.
pub fn make_audit_file() -> vfs::RamFile {
	let buf = read_audit_records();
	vfs::RamFile::new_with_data(&buf)
}

fn read_audit_records() -> Vec<u8> {
	let records = capability::global_audit().read_from(0, 64);
	if records.records.is_empty() {
		return Vec::new();
	}

	let mut buf = Vec::with_capacity(64 * 100);
	let mut off = 0usize;

	// next_cursor (u64)
	buf.extend_from_slice(&records.next_cursor.to_le_bytes());
	off += 8;

	// gap (u8) + count (u16) + reserved (u16)
	buf.push(if records.gap { 1 } else { 0 });
	buf.extend_from_slice(&(records.records.len() as u16).to_le_bytes());
	buf.extend_from_slice(&0u16.to_le_bytes());
	off += 6;

	// Pad to 8 bytes
	while off % 8 != 0 {
		buf.push(0);
		off += 1;
	}

	// Entries
	for record in &records.records {
		buf.extend_from_slice(&record.sequence.to_le_bytes()); // 8
		buf.extend_from_slice(&record.ticks.to_le_bytes()); // 8
		buf.extend_from_slice(&record.cpu.to_le_bytes()); // 4
		buf.extend_from_slice(&record.caller.to_le_bytes()); // 8

		let op = record.operation.as_bytes();
		buf.push(op.len().min(31) as u8);
		buf.extend_from_slice(&op[..op.len().min(31)]);
		buf.extend_from_slice(&[0u8; 31]); // pad

		let ok = record.object_kind.as_bytes();
		buf.push(ok.len().min(31) as u8);
		buf.extend_from_slice(&ok[..ok.len().min(31)]);
		buf.extend_from_slice(&[0u8; 31]); // pad

		buf.extend_from_slice(&record.object_id.to_le_bytes()); // 8
		buf.extend_from_slice(&record.requested_rights.to_le_bytes()); // 4
		buf.push(match record.result { capability::AuditResult::Ok => 0, capability::AuditResult::Denied(code) => code + 1 }); // 1
		buf.extend_from_slice(&record.parent_generation.to_le_bytes()); // 4
		buf.extend_from_slice(&0u32.to_le_bytes()); // reserved

		off += 96;
	}

	// Update next_cursor in header
	buf[..8].copy_from_slice(&records.next_cursor.to_le_bytes());

	buf
}
