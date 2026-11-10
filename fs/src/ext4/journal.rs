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
	 *   2. Write journal header with block count
	 *   3. Write all buffered metadata blocks to journal
	 *   4. Write commit record (sequence number)
	 *   5. Update journal superblock
	 */
	pub fn commit_transaction(&mut self, dev: &dyn BlockDev) {
		if let Some(ref mut tx) = self.current_tx {
			tx.t_state = JournalState::Committing;

			let num_buffers = tx.t_buffers.len();
			let total_blocks = 1 + num_buffers + 1; /* header + buffers + commit */

			/* Write journal transaction header */
			let header_block = self.sb.j_head;
			let spb = self.sb.j_blocksize / 512;
			let header_sector =
				(self.start_block as u64 + header_block as u64) * (spb as u64);
			let mut header_buf = [0u8; 512];
			header_buf[0..4].copy_from_slice(&JBD2_MAGIC.to_le_bytes()); /* magic */
			header_buf[4..8].copy_from_slice(&total_blocks.to_le_bytes()); /* block count */
			header_buf[8..12].copy_from_slice(&tx.t_id.to_le_bytes()); /* sequence */
			for s in 0..spb as usize {
				dev.write_block(header_sector + s as u64, &header_buf);
			}

			/* Write each buffered block to the journal */
			let mut cur_block = (header_block + 1) % self.size;
			for buf in tx.t_buffers.iter() {
				let sector = (self.start_block as u64 + cur_block as u64) * (spb as u64);
				let mut journal_buf = [0u8; 512];
				/* Original block number (4 bytes) */
				journal_buf[0..4].copy_from_slice(&buf.block.to_le_bytes());
				/* Block data */
				let copy_len = core::cmp::min(buf.data.len(), 512);
				journal_buf[4..4 + copy_len].copy_from_slice(&buf.data[..copy_len]);
				for s in 0..spb as usize {
					let off = s * 512;
					let end = core::cmp::min(4 + 512, 4 + buf.data.len());
					let slice_start = off;
					let slice_end = core::cmp::min(512, end - off);
					if slice_start < slice_end && slice_end <= buf.data.len() {
						journal_buf[4 + slice_start..4 + slice_end]
							.copy_from_slice(&buf.data[slice_start..slice_end]);
					}
					dev.write_block(sector + s as u64, &journal_buf);
				}
				cur_block = (cur_block + 1) % self.size;
			}

			/* Write commit record (next block after buffers) */
			let commit_block = cur_block;
			let commit_sector =
				(self.start_block as u64 + commit_block as u64) * (spb as u64);
			let mut commit_buf = [0u8; 512];
			commit_buf[0..4].copy_from_slice(&tx.t_id.to_le_bytes()); /* Sequence number */
			for s in 0..spb as usize {
				dev.write_block(commit_sector + s as u64, &commit_buf);
			}

			/* Update journal superblock */
			self.sb.j_head = (commit_block + 1) % self.size;
			self.sb.j_tail = header_block;
			self.sb.j_free = self.size - total_blocks as u32;
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

	/* Recover from a crash — scan journal for committed transactions and replay */
	pub fn recover(&mut self, dev: &dyn BlockDev) {
		if !self.needs_recovery() {
			return;
		}

		#[cfg(feature = "kernel")]
		hal::serial_println!("[journal] Recovery needed, scanning journal...");

		/* Read the journal superblock to get current head/tail */
		let head = self.sb.j_head;
		let tail = self.sb.j_tail;

		/* If head == tail, journal is empty */
		if head == tail {
			self.sb.j_state = JournalState::Committed as u8;
			self.write_superblock(dev);
			return;
		}

		/* Scan the journal area for valid transaction headers.
		 * We walk from head backwards, looking for JBD2_MAGIC. */
		let spb = self.sb.j_blocksize / 512;
		let mut seq_start: Option<u32> = None;
		let mut seq_end: u32 = head;

		/* Walk backwards through the journal to find the last valid transaction */
		let mut pos = head;
		let mut iterations = self.size * 2; /* safety limit */
		while iterations > 0 {
			iterations -= 1;
			let sector = (self.start_block as u64 + pos as u64) * (spb as u64);
			let mut buf = [0u8; 512];
			if !dev.read_block(sector, &mut buf) {
				break;
			}

			let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
			if magic == JBD2_MAGIC {
				let block_count = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
				let seq = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
				if block_count >= 2 && block_count <= self.size {
					seq_start = Some(seq);
					let commit_pos = (pos + 1 + block_count - 1) % self.size;
					/* Verify commit record follows */
					let commit_sector =
						(self.start_block as u64 + commit_pos as u64) * (spb as u64);
					let mut commit_buf = [0u8; 512];
					if dev.read_block(commit_sector, &mut commit_buf) {
						let commit_seq =
							u32::from_le_bytes([commit_buf[0], commit_buf[1], commit_buf[2], commit_buf[3]]);
						if commit_seq == seq {
							seq_end = commit_pos;
							break; /* Found last valid transaction */
						}
					}
				}
			}

			pos = if pos == 0 { self.size - 1 } else { pos - 1 };
		}

		/* Replay the last valid transaction if found */
		if let Some(seq) = seq_start {
			#[cfg(feature = "kernel")]
			hal::serial_println!("[journal] Replaying transaction seq={}", seq);
			/* Walk forward from where we found the header to replay blocks */
			/* We need to re-scan forward from the tail area */
			let mut scan_pos = tail;
			let mut found_header = false;
			let mut scan_iters = self.size;
			while scan_iters > 0 {
				scan_iters -= 1;
				let sector = (self.start_block as u64 + scan_pos as u64) * (spb as u64);
				let mut buf = [0u8; 512];
				if !dev.read_block(sector, &mut buf) {
					break;
				}
				let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
				if magic == JBD2_MAGIC {
					let block_count = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
					let tx_seq = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
					if tx_seq == seq {
						found_header = true;
						/* Replay each buffered block in this transaction */
						let mut cur_block = (scan_pos + 1) % self.size;
						for _ in 0..(block_count - 2) {
							/* Skip header(1) and commit(1), read buffer blocks */
							let buf_sector =
								(self.start_block as u64 + cur_block as u64) * (spb as u64);
							let mut journal_buf = [0u8; 512];
							if dev.read_block(buf_sector, &mut journal_buf) {
								let orig_block =
									u32::from_le_bytes([journal_buf[0], journal_buf[1], journal_buf[2], journal_buf[3]]);
								/* Write the replayed data to the original block location */
								let bsz = self.sb.j_blocksize as usize;
								let sector = (self.start_block as u64 + orig_block as u64) * (spb as u64);
								for s in 0..spb as usize {
									let chunk_end = core::cmp::min(512, bsz - s * 512);
									if s * 512 < bsz {
										let mut write_buf = [0u8; 512];
										let src_start = 4 + s * 512;
										let src_end = core::cmp::min(516, src_start + chunk_end);
										let copy_len = core::cmp::min(chunk_end, journal_buf.len().saturating_sub(src_start));
										if copy_len > 0 {
											write_buf[..copy_len]
												.copy_from_slice(&journal_buf[src_start..src_start + copy_len]);
										}
										dev.write_block(sector + s as u64, &write_buf);
									}
								}
								#[cfg(feature = "kernel")]
								hal::serial_println!(
									"[journal]   replayed block {} -> sector {}",
									orig_block,
									sector
								);
							}
							cur_block = (cur_block + 1) % self.size;
						}
						break;
					}
				}
				scan_pos = (scan_pos + 1) % self.size;
			}

			if found_header {
				#[cfg(feature = "kernel")]
				hal::serial_println!("[journal] Recovery complete, clearing journal");
			} else {
				#[cfg(feature = "kernel")]
				hal::serial_println!("[journal] Recovery: could not find transaction in journal");
			}
		}

		/* Clear journal state */
		self.sb.j_state = JournalState::Committed as u8;
		self.sb.j_tail = 0;
		self.sb.j_head = 0;
		self.write_superblock(dev);
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
