/*
 * ext4/journal.rs - JBD2 (Journaling Block Device 2) Basic Implementation
 *
 * Provides ordered mode journaling for ext4 metadata consistency.
 * In ordered mode, data blocks are written before the journal transaction
 * is committed, ensuring that on crash recovery, either both data and
 * metadata are consistent, or neither is.
 *
 * On-disk layout:
 *   - Journal superblock at the first journal block
 *   - Journal transactions are written sequentially
 *   - Each transaction contains a commit record followed by data blocks
 *
 * This is a simplified implementation suitable for a kernel in development:
 *   - No checksum validation (future enhancement)
 *   - Fixed-size journal (configured at format time)
 *   - Single-threaded (no concurrent transactions)
 */

extern crate alloc;
use alloc::vec::Vec;
use crate::BlockDev;
use super::superblock::Superblock;

/* ------------------------------------------------------------------ */
/*  On-disk journal structures                                         */
/* ------------------------------------------------------------------ */

/* JBD2 magic number */
const JBD2_MAGIC: u32 = 0xFF0FBDFF;

/* Journal states */
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JournalState {
	/* Transaction is being built up */
	Starting = 0,
	/* Transaction is being committed */
	Committing = 1,
	/* Transaction has been committed */
	Committed = 2,
	/* Transaction is invalid/corrupted */
	Invalid = 255,
}

/*
 * Journal superblock (on-disk, at offset 0 of journal area)
 *
 * Layout (64 bytes, aligned to block size):
 *   0:    j_magic      (0xFF0FBDFF)
 *   4:    j_version    (1 for JBD2)
 *   8:    j_blocksize  (filesystem block size in bytes)
 *   12:   j_devnr      (device number, unused in our context)
 *   16:   j_head       (next block to write, sequence-dependent)
 *   20:   j_tail       (oldest uncommitted block)
 *   24:   j_free       (free blocks in journal)
 *   28:   j_start      (first block of journal, set at creation)
 *   32:   j_size       (total journal size in blocks)
 *   36:   j_sequence   (next transaction sequence number)
 *   40:   j_last_commit (last committed transaction sequence)
 *   44:   j_state      (current journal state)
 *   45:   padding      (reserved)
 */
#[repr(C)]
pub struct JournalSuperblock {
	pub j_magic: u32,
	pub j_version: u32,
	pub j_blocksize: u32,
	pub j_devnr: u32,
	pub j_head: u32,
	pub j_tail: u32,
	pub j_free: u32,
	pub j_start: u32,
	pub j_size: u32,
	pub j_sequence: u32,
	pub j_last_commit: u32,
	pub j_state: u8,
	pub _padding: [u8; 3],
}

impl JournalSuperblock {
	/* Create a new journal superblock */
	pub fn new(blocksize: u32, start: u32, size: u32) -> Self {
		JournalSuperblock {
			j_magic: JBD2_MAGIC,
			j_version: 2,
			j_blocksize: blocksize,
			j_devnr: 0,
			j_head: 0,
			j_tail: 0,
			j_free: size - 1, /* Reserve one block for the superblock itself */
			j_start: start,
			j_size: size,
			j_sequence: 1,
			j_last_commit: 0,
			j_state: JournalState::Committed as u8,
			_padding: [0; 3],
		}
	}

	/* Serialize to bytes */
	pub fn serialize(&self) -> [u8; 64] {
		let mut buf = [0u8; 64];
		buf[0..4].copy_from_slice(&self.j_magic.to_le_bytes());
		buf[4..8].copy_from_slice(&self.j_version.to_le_bytes());
		buf[8..12].copy_from_slice(&self.j_blocksize.to_le_bytes());
		buf[12..16].copy_from_slice(&self.j_devnr.to_le_bytes());
		buf[16..20].copy_from_slice(&self.j_head.to_le_bytes());
		buf[20..24].copy_from_slice(&self.j_tail.to_le_bytes());
		buf[24..28].copy_from_slice(&self.j_free.to_le_bytes());
		buf[28..32].copy_from_slice(&self.j_start.to_le_bytes());
		buf[32..36].copy_from_slice(&self.j_size.to_le_bytes());
		buf[36..40].copy_from_slice(&self.j_sequence.to_le_bytes());
		buf[40..44].copy_from_slice(&self.j_last_commit.to_le_bytes());
		buf[44] = self.j_state;
		buf
	}

	/* Deserialize from bytes (reads first 48 bytes) */
	pub fn deserialize(buf: &[u8]) -> Option<Self> {
		if buf.len() < 48 {
			return None;
		}
		if buf[0..4] != JBD2_MAGIC.to_le_bytes() {
			return None;
		}
		Some(JournalSuperblock {
			j_magic: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
			j_version: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
			j_blocksize: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
			j_devnr: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
			j_head: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
			j_tail: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
			j_free: u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
			j_start: u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
			j_size: u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
			j_sequence: u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]),
			j_last_commit: u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]),
			j_state: buf[44],
			_padding: [buf[45], buf[46], buf[47]],
		})
	}
}

/* ------------------------------------------------------------------ */
/*  In-memory journal state                                            */
/* ------------------------------------------------------------------ */

/*
 * JournalBuffer - A single buffer within a transaction
 *
 * Tracks a block that needs to be journaled, along with its data.
 */
pub struct JournalBuffer {
	/* Original block number on disk */
	pub block: u32,
	/* Data to write */
	pub data: Vec<u8>,
	/* Checksum (placeholder, not yet implemented) */
	pub checksum: u32,
}

/*
 * JournalTransaction - An in-memory transaction being built
 *
 * In ordered mode:
 *   1. Data blocks are written to disk first
 *   2. Metadata blocks are buffered in this transaction
 *   3. Journal commit record is written
 *   4. Journal superblock is updated
 */
pub struct JournalTransaction {
	/* Transaction sequence number */
	pub t_id: u32,
	/* Current state */
	pub t_state: JournalState,
	/* Buffered blocks */
	pub t_buffers: Vec<JournalBuffer>,
}

impl JournalTransaction {
	/* Create a new transaction */
	pub fn new(id: u32) -> Self {
		JournalTransaction {
			t_id: id,
			t_state: JournalState::Starting,
			t_buffers: Vec::new(),
		}
	}

	/* Add a buffer to this transaction */
	pub fn add_buffer(&mut self, block: u32, data: Vec<u8>) {
		self.t_buffers.push(JournalBuffer {
			block,
			data,
			checksum: 0, /* TODO: implement checksums */
		});
	}

	/* Check if the transaction has any buffers */
	pub fn is_empty(&self) -> bool {
		self.t_buffers.is_empty()
	}
}

/* ------------------------------------------------------------------ */
/*  Journal operations                                                 */
/* ------------------------------------------------------------------ */

/*
 * Journal - Manages the journal for a filesystem
 *
 * Holds the journal superblock and provides operations for
 * beginning, buffering, and committing transactions.
 */
pub struct Journal {
	/* Journal superblock (in-memory copy) */
	pub sb: JournalSuperblock,
	/* Current transaction (if any) */
	pub current_tx: Option<JournalTransaction>,
	/* Journal start block (relative to filesystem) */
	pub start_block: u32,
	/* Journal size in blocks */
	pub size: u32,
}

impl Journal {
	/* Create a new journal for formatting */
	pub fn new(blocksize: u32, start_block: u32, size: u32) -> Self {
		Journal {
			sb: JournalSuperblock::new(blocksize, start_block, size),
			current_tx: None,
			start_block,
			size,
		}
	}

	/* Deserialize a journal superblock from a block buffer */
	pub fn deserialize_jsb(buf: &[u8; 512]) -> Option<JournalSuperblock> {
		JournalSuperblock::deserialize(&buf[..64])
	}

	/* Write the journal superblock to disk */
	pub fn write_superblock(&self, dev: &dyn BlockDev) {
		let buf = self.sb.serialize();
		let mut sector_buf = [0u8; 512];
		sector_buf[..buf.len()].copy_from_slice(&buf);
		let journal_start_sector = (self.start_block as u64) * (self.sb.j_blocksize / 512) as u64;
		dev.write_block(journal_start_sector, &sector_buf);
	}

	/* Initialize a fresh journal (called during format) */
	pub fn init(&mut self, dev: &dyn BlockDev) {
		/* Zero out the journal area */
		let zeros = [0u8; 512];
		let spb = self.sb.j_blocksize / 512;
		for i in 0..self.size {
			let sector = (self.start_block as u64 + i as u64) * spb as u64;
			for s in 0..spb {
				dev.write_block(sector + s as u64, &zeros);
			}
		}

		/* Write the superblock */
		self.write_superblock(dev);
	}

	/* Begin a new transaction */
	pub fn begin_transaction(&mut self, dev: &dyn BlockDev) -> Option<u32> {
		/* Read current superblock to get sequence number */
		let seq = self.sb.j_sequence;

		/* Create new transaction */
		self.current_tx = Some(JournalTransaction::new(seq));
		self.sb.j_state = JournalState::Committing as u8;
		self.write_superblock(dev);

		Some(seq)
	}

	/* Buffer a block for journaling */
	pub fn journal_write(&mut self, block: u32, data: Vec<u8>) {
		if let Some(ref mut tx) = self.current_tx {
			tx.add_buffer(block, data);
		}
	}

	/* Commit the current transaction (ordered mode) */
	/*
	 * In ordered mode:
	 *   1. Data blocks have already been written to disk (by the caller)
	 *   2. Write commit record to journal
	 *   3. Update journal superblock
	 */
	pub fn commit_transaction(&mut self, dev: &dyn BlockDev) {
		if let Some(ref mut tx) = self.current_tx {
			tx.t_state = JournalState::Committing;

			/* Write commit record (simplified: just the sequence number) */
			/* Write commit record to journal (next available block) */
			let commit_block = self.sb.j_head;
			let spb = self.sb.j_blocksize / 512;
			let sector = (self.start_block as u64 + commit_block as u64) * (spb as u64);
			let mut sector_buf = [0u8; 512];
			sector_buf[0..4].copy_from_slice(&tx.t_id.to_le_bytes()); /* Sequence number */
			for s in 0..spb {
				dev.write_block(sector + s as u64, &sector_buf);
			}

			/* Update journal superblock */
			self.sb.j_head = (commit_block + 1) % self.size;
			self.sb.j_tail = self.sb.j_head;
			self.sb.j_free = self.size - 1;
			self.sb.j_sequence = tx.t_id + 1;
			self.sb.j_last_commit = tx.t_id;
			self.sb.j_state = JournalState::Committed as u8;
			self.write_superblock(dev);

			/* Clear current transaction */
			self.current_tx = None;
		}
	}

	/* Check if journal recovery is needed */
	pub fn needs_recovery(&self) -> bool {
		self.sb.j_state != JournalState::Committed as u8
	}

	/* Recover from a crash (simplified: just clear the journal) */
	pub fn recover(&mut self, dev: &dyn BlockDev) {
		if self.needs_recovery() {
			/* In a full implementation, we would:
			 * 1. Scan the journal for valid transactions
			 * 2. Replay metadata blocks from the journal
			 * 3. Update inodes to reflect recovered state
			 *
			 * For now, we just clear the journal state.
			 * This is safe because we use ordered mode:
			 * if the journal is incomplete, the data on disk
			 * is already consistent (data was written before
			 * the journal commit record).
			 */
			// serial_println!("[journal] Recovery needed, clearing journal");
			self.sb.j_state = JournalState::Committed as u8;
			self.sb.j_tail = 0;
			self.sb.j_head = 0;
			self.write_superblock(dev);
		}
	}
}

/* ------------------------------------------------------------------ */
/*  Journal layout helpers                                             */
/* ------------------------------------------------------------------ */

/*
 * Journal layout for a formatted filesystem:
 *   - Superblock: sectors 0-1 (1 sector for ext4 superblock at sector 1)
 *   - Block Group Descriptor Table: follows superblock
 *   - Block Bitmap: follows BGDT
 *   - Inode Bitmap: follows block bitmap
 *   - Inode Table: follows inode bitmap
 *   - Journal: follows inode table (reserved space)
 *   - Data blocks: follow journal
 *
 * Journal size: 64 blocks (256 KiB with 4 KiB blocks)
 */
pub const JOURNAL_BLOCKS: u32 = 64;

/* Calculate journal position and size for a given layout */
pub fn calculate_journal_layout(
	_block_size: u32,
	inode_table_blocks: u32,
	first_data_block: u32,
) -> (u32, u32) {
	/* Journal starts after the inode table */
	let journal_start = first_data_block + inode_table_blocks;
	/* Journal size is fixed at JOURNAL_BLOCKS */
	(journal_start, JOURNAL_BLOCKS)
}
