/*
 * block.rs - Block device VFS INode wrapper
 *
 * Provides a VFS-compatible interface over the VirtIO block device,
 * translating byte-oriented read/write calls into sector operations.
 */

use vfs::{FileType, INode};

/*
 * struct BlockDevice - VFS INode backed by VirtIO block device
 *
 * Translates byte offsets to sector numbers and performs sector-aligned I/O.
 * Partial-sector reads return only the requested bytes. Partial-sector
 * writes use read-modify-write.
 */
pub struct BlockDevice;

impl BlockDevice {
	pub fn new() -> Self {
		Self
	}
}

impl INode for BlockDevice {
	/*
	 * read - Read bytes from the block device
	 * @offset: Byte offset to start reading from
	 * @buf:    Buffer to fill with data
	 *
	 * Return: Number of bytes read
	 */
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		let blk = match crate::virtio::virtio_blk() {
			Some(b) => b,
			None => return 0,
		};
		let mut dev = blk.lock();
		let capacity_bytes = (dev.capacity() * 512) as usize;

		if offset >= capacity_bytes || buf.is_empty() {
			return 0;
		}

		let end = core::cmp::min(offset + buf.len(), capacity_bytes);
		let mut pos = offset;
		let mut written = 0;
		let mut sector_buf = [0u8; 512];

		while pos < end {
			let sector = (pos / 512) as u64;
			let off_in_sector = pos % 512;
			let chunk = core::cmp::min(512 - off_in_sector, end - pos);

			if dev.read_sector(sector, &mut sector_buf).is_err() {
				break;
			}

			buf[written..written + chunk]
				.copy_from_slice(&sector_buf[off_in_sector..off_in_sector + chunk]);
			pos += chunk;
			written += chunk;
		}

		written
	}

	/*
	 * write - Write bytes to the block device
	 * @offset: Byte offset to start writing at
	 * @buf:    Data to write
	 *
	 * Return: Number of bytes written
	 */
	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		let blk = match crate::virtio::virtio_blk() {
			Some(b) => b,
			None => return 0,
		};
		let mut dev = blk.lock();
		let capacity_bytes = (dev.capacity() * 512) as usize;

		if offset >= capacity_bytes || buf.is_empty() {
			return 0;
		}

		let end = core::cmp::min(offset + buf.len(), capacity_bytes);
		let mut pos = offset;
		let mut consumed = 0;
		let mut sector_buf = [0u8; 512];

		while pos < end {
			let sector = (pos / 512) as u64;
			let off_in_sector = pos % 512;
			let chunk = core::cmp::min(512 - off_in_sector, end - pos);

			if chunk < 512 {
				/* Partial sector: read-modify-write */
				if dev.read_sector(sector, &mut sector_buf).is_err() {
					break;
				}
			}

			sector_buf[off_in_sector..off_in_sector + chunk]
				.copy_from_slice(&buf[consumed..consumed + chunk]);

			if dev.write_sector(sector, &sector_buf).is_err() {
				break;
			}

			pos += chunk;
			consumed += chunk;
		}

		consumed
	}

	fn metadata(&self) -> FileType {
		FileType::Device
	}

	fn size(&self) -> usize {
		match crate::virtio::virtio_blk() {
			Some(b) => (b.lock().capacity() * 512) as usize,
			None => 0,
		}
	}
}
