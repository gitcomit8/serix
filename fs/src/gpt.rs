/*
 * gpt.rs - GUID Partition Table parsing
 *
 * Reads and validates a GPT from any sector source that implements
 * SectorReader. Validation covers the header signature, revision, header
 * size, the header CRC32, the partition-entry-array CRC32, and entry bounds
 * against the disk capacity. A table that fails any check is rejected as a
 * whole — partial or damaged tables are never exposed to the VFS.
 *
 * The module is deliberately free of kernel dependencies (only `alloc`), so
 * the parser is unit-testable on the host. The tests at the bottom of this
 * file are exercised by a host harness that includes it directly:
 *
 *   mkdir -p target/gpt-host-tests/src
 *   printf '[package]\nname="gpt-host-tests"\nversion="0.0.0"\nedition="2024"\n[workspace]\n' \
 *     > target/gpt-host-tests/Cargo.toml
 *   printf 'extern crate alloc;\n#[path="../../../fs/src/gpt.rs"] mod gpt;\n' \
 *     > target/gpt-host-tests/src/lib.rs
 *   cargo test --manifest-path target/gpt-host-tests/Cargo.toml \
 *     --target "$(rustc -vV | awk '/host:/{print $2}')"
 */

use alloc::vec::Vec;

pub const SECTOR_SIZE: usize = 512;

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_REVISION_1_0: u32 = 0x0001_0000;
const HEADER_MIN_SIZE: usize = 92;
const HEADER_CRC_OFFSET: usize = 16;
const MAX_ENTRY_COUNT: u32 = 1024;
const MIN_ENTRY_SIZE: u32 = 128;
const MAX_ENTRY_ARRAY_BYTES: usize = 1024 * 1024;

/*
 * trait SectorReader - Minimal sector source for partition-table parsing
 *
 * Implemented for the kernel's BlockDev. Kept narrow so the parser can be
 * driven by an in-memory image in host tests.
 */
pub trait SectorReader {
	fn read_sector(&self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GptError {
	/* The underlying sector source failed a read */
	Io,
	/* No "EFI PART" signature at LBA 1 */
	NoGpt,
	/* Revision is not 1.0 */
	BadRevision,
	/* Header size is outside 92..=512 */
	BadHeaderSize,
	/* Header CRC32 does not match the header contents */
	BadHeaderCrc,
	/* Entry count/size is out of range or the array does not fit */
	BadEntryArray,
	/* Entry-array CRC32 does not match the array contents */
	BadEntryArrayCrc,
}

/*
 * struct Partition - One validated, non-empty GPT entry
 * @number:      1-based partition number (drives /dev/sdXN)
 * @start_lba:   First sector of the partition
 * @sector_count: Length in 512-byte sectors, clipped to the disk capacity
 * @type_guid:   Partition type GUID in on-disk byte order
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Partition {
	pub number: u32,
	pub start_lba: u64,
	pub sector_count: u64,
	pub type_guid: [u8; 16],
}

/*
 * crc32 - CRC-32/ISO-HDLC (the polynomial GPT uses)
 * @data: Bytes to checksum
 *
 * Return: CRC value
 */
pub fn crc32(data: &[u8]) -> u32 {
	let mut crc = 0xFFFF_FFFFu32;
	for &byte in data {
		crc ^= byte as u32;
		for _ in 0..8 {
			let mask = (crc & 1).wrapping_neg();
			crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
		}
	}
	!crc
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
	u32::from_le_bytes([
		buf[offset],
		buf[offset + 1],
		buf[offset + 2],
		buf[offset + 3],
	])
}

fn read_u64(buf: &[u8], offset: usize) -> u64 {
	let mut bytes = [0u8; 8];
	bytes.copy_from_slice(&buf[offset..offset + 8]);
	u64::from_le_bytes(bytes)
}

/*
 * read_gpt - Parse and validate the primary GPT
 * @dev:          Sector source containing the disk
 * @disk_sectors: Device capacity in 512-byte sectors (0 = unknown)
 *
 * Return: Ok(partitions) with non-empty entries in table order, or the first
 *         validation failure. Zero-length entries and entries past the end of
 *         the disk are skipped.
 */
pub fn read_gpt<R: SectorReader + ?Sized>(
	dev: &R,
	disk_sectors: u64,
) -> Result<Vec<Partition>, GptError> {
	let mut header = [0u8; SECTOR_SIZE];
	/* LBA 0 is the protective MBR; the primary GPT header is LBA 1 */
	if !dev.read_sector(1, &mut header) {
		return Err(GptError::Io);
	}

	if &header[0..8] != GPT_SIGNATURE {
		return Err(GptError::NoGpt);
	}

	if read_u32(&header, 8) != GPT_REVISION_1_0 {
		return Err(GptError::BadRevision);
	}

	let header_size = read_u32(&header, 12) as usize;
	if !(HEADER_MIN_SIZE..=SECTOR_SIZE).contains(&header_size) {
		return Err(GptError::BadHeaderSize);
	}

	/* CRC covers the header with the CRC field itself zeroed */
	let stored_header_crc = read_u32(&header, HEADER_CRC_OFFSET);
	let mut crc_header = [0u8; SECTOR_SIZE];
	crc_header[..header_size].copy_from_slice(&header[..header_size]);
	crc_header[HEADER_CRC_OFFSET..HEADER_CRC_OFFSET + 4].fill(0);
	if crc32(&crc_header[..header_size]) != stored_header_crc {
		return Err(GptError::BadHeaderCrc);
	}

	let entries_lba = read_u64(&header, 72);
	let entry_count = read_u32(&header, 80);
	let entry_size = read_u32(&header, 84);
	let stored_entries_crc = read_u32(&header, 88);

	if entry_count == 0
		|| entry_count > MAX_ENTRY_COUNT
		|| entry_size < MIN_ENTRY_SIZE
		|| entry_size % 8 != 0
	{
		return Err(GptError::BadEntryArray);
	}

	let array_bytes = entry_count as usize * entry_size as usize;
	if array_bytes > MAX_ENTRY_ARRAY_BYTES {
		return Err(GptError::BadEntryArray);
	}

	/* Read the entry array, then CRC exactly the declared entry bytes */
	let padded_bytes = array_bytes.div_ceil(SECTOR_SIZE) * SECTOR_SIZE;
	let mut array = alloc::vec![0u8; padded_bytes];
	let mut sector = [0u8; SECTOR_SIZE];
	for (index, chunk) in array.chunks_mut(SECTOR_SIZE).enumerate() {
		if !dev.read_sector(entries_lba + index as u64, &mut sector) {
			return Err(GptError::Io);
		}
		chunk.copy_from_slice(&sector);
	}

	if crc32(&array[..array_bytes]) != stored_entries_crc {
		return Err(GptError::BadEntryArrayCrc);
	}

	let mut partitions = Vec::new();
	for entry_index in 0..entry_count as usize {
		let start = entry_index * entry_size as usize;
		let entry = &array[start..start + entry_size as usize];

		/* An all-zero type GUID marks an unused entry */
		if entry[..16].iter().all(|&b| b == 0) {
			continue;
		}

		let first_lba = read_u64(entry, 32);
		let last_lba = read_u64(entry, 40);
		if last_lba < first_lba || first_lba == 0 {
			continue;
		}

		let mut end_lba = last_lba;
		if disk_sectors != 0 {
			if first_lba >= disk_sectors {
				continue;
			}
			/* A partition that claims past the end is clipped, not trusted */
			end_lba = core::cmp::min(last_lba, disk_sectors - 1);
		}

		let mut type_guid = [0u8; 16];
		type_guid.copy_from_slice(&entry[..16]);

		partitions.push(Partition {
			number: partitions.len() as u32 + 1,
			start_lba: first_lba,
			sector_count: end_lba - first_lba + 1,
			type_guid,
		});
	}

	Ok(partitions)
}

#[cfg(test)]
mod tests {
	use super::*;

	/* In-memory disk image used as a sector source */
	struct Image(Vec<u8>);

	impl SectorReader for Image {
		fn read_sector(&self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> bool {
			let start = lba as usize * SECTOR_SIZE;
			let end = start + SECTOR_SIZE;
			if end > self.0.len() {
				return false;
			}
			buf.copy_from_slice(&self.0[start..end]);
			true
		}
	}

	const TYPE_LINUX: [u8; 16] = [
		0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D,
		0xE4,
	];

	/*
	 * build_image - Create a GPT disk image with the given partition ranges
	 * @ranges: (first_lba, last_lba) pairs, written as Linux data partitions
	 * @capacity_sectors: Image size in sectors
	 */
	fn build_image(ranges: &[(u64, u64)], capacity_sectors: usize) -> Image {
		let entry_count: u32 = 128;
		let entry_size: u32 = 128;
		let entries_lba: u64 = 2;
		let array_bytes = (entry_count * entry_size) as usize;
		let array_sectors = array_bytes.div_ceil(SECTOR_SIZE);
		let mut bytes = alloc::vec![0u8; capacity_sectors * SECTOR_SIZE];

		/* Protective MBR (only the type byte matters to the parser) */
		bytes[450] = 0xEE;

		let mut entries = alloc::vec![0u8; array_bytes];
		for (index, &(first_lba, last_lba)) in ranges.iter().enumerate() {
			let entry =
				&mut entries[index * entry_size as usize..(index + 1) * entry_size as usize];
			entry[..16].copy_from_slice(&TYPE_LINUX);
			entry[16..32].copy_from_slice(&[index as u8 + 1; 16]);
			entry[32..40].copy_from_slice(&first_lba.to_le_bytes());
			entry[40..48].copy_from_slice(&last_lba.to_le_bytes());
		}
		let entries_crc = crc32(&entries);

		let entry_start = entries_lba as usize * SECTOR_SIZE;
		bytes[entry_start..entry_start + array_bytes].copy_from_slice(&entries);

		/* Header at LBA 1 */
		let hdr = 1 * SECTOR_SIZE;
		bytes[hdr..hdr + 8].copy_from_slice(GPT_SIGNATURE);
		bytes[hdr + 8..hdr + 12].copy_from_slice(&GPT_REVISION_1_0.to_le_bytes());
		bytes[hdr + 12..hdr + 16].copy_from_slice(&(HEADER_MIN_SIZE as u32).to_le_bytes());
		bytes[hdr + 24..hdr + 32].copy_from_slice(&1u64.to_le_bytes());
		bytes[hdr + 32..hdr + 40]
			.copy_from_slice(&((entries_lba + array_sectors as u64) as u64).to_le_bytes());
		bytes[hdr + 40..hdr + 48].copy_from_slice(&34u64.to_le_bytes());
		bytes[hdr + 48..hdr + 56].copy_from_slice(&((capacity_sectors - 34) as u64).to_le_bytes());
		bytes[hdr + 72..hdr + 80].copy_from_slice(&entries_lba.to_le_bytes());
		bytes[hdr + 80..hdr + 84].copy_from_slice(&entry_count.to_le_bytes());
		bytes[hdr + 84..hdr + 88].copy_from_slice(&entry_size.to_le_bytes());
		bytes[hdr + 88..hdr + 92].copy_from_slice(&entries_crc.to_le_bytes());

		let header_crc = crc32(&bytes[hdr..hdr + HEADER_MIN_SIZE]);
		bytes[hdr + 16..hdr + 20].copy_from_slice(&header_crc.to_le_bytes());

		Image(bytes)
	}

	#[test]
	fn crc32_matches_known_vector() {
		assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
		assert_eq!(crc32(b""), 0);
	}

	#[test]
	fn parses_partitions_with_offsets() {
		let image = build_image(&[(2048, 4095), (4096, 8191)], 8192);
		let parts = read_gpt(&image, 8192).expect("valid GPT must parse");

		assert_eq!(parts.len(), 2);
		assert_eq!(parts[0].number, 1);
		assert_eq!(parts[0].start_lba, 2048);
		assert_eq!(parts[0].sector_count, 2048);
		assert_eq!(parts[0].type_guid, TYPE_LINUX);
		assert_eq!(parts[1].number, 2);
		assert_eq!(parts[1].start_lba, 4096);
		assert_eq!(parts[1].sector_count, 4096);
	}

	#[test]
	fn skips_empty_entries() {
		let image = build_image(&[(2048, 4095), (0, 0), (8192, 12287)], 16384);
		let parts = read_gpt(&image, 16384).expect("valid GPT must parse");
		assert_eq!(parts.len(), 2);
		assert_eq!(parts[0].start_lba, 2048);
		assert_eq!(parts[1].start_lba, 8192);
		/* Partition numbers stay contiguous even with a gap in the table */
		assert_eq!(parts[1].number, 2);
	}

	#[test]
	fn rejects_corrupt_header_crc() {
		let mut image = build_image(&[(2048, 4095)], 8192);
		image.0[SECTOR_SIZE + 12] ^= 0x01; /* header_size -> CRC mismatch */
		assert_eq!(read_gpt(&image, 8192), Err(GptError::BadHeaderCrc));
	}

	#[test]
	fn rejects_corrupt_entry_array() {
		let mut image = build_image(&[(2048, 4095)], 8192);
		image.0[2 * SECTOR_SIZE + 40] ^= 0x01; /* last_lba byte */
		assert_eq!(read_gpt(&image, 8192), Err(GptError::BadEntryArrayCrc));
	}

	#[test]
	fn rejects_missing_signature() {
		let mut image = build_image(&[(2048, 4095)], 8192);
		image.0[SECTOR_SIZE] = b'X';
		assert_eq!(read_gpt(&image, 8192), Err(GptError::NoGpt));
	}

	#[test]
	fn rejects_bad_revision() {
		let mut image = build_image(&[(2048, 4095)], 8192);
		image.0[SECTOR_SIZE + 8] = 0x02;
		assert_eq!(read_gpt(&image, 8192), Err(GptError::BadRevision));
	}

	#[test]
	fn rejects_oversized_header() {
		let mut image = build_image(&[(2048, 4095)], 8192);
		image.0[SECTOR_SIZE + 12..SECTOR_SIZE + 16].copy_from_slice(&513u32.to_le_bytes());
		/* The size check fires before the CRC check */
		assert_eq!(read_gpt(&image, 8192), Err(GptError::BadHeaderSize));
	}

	#[test]
	fn rejects_unreadable_sector() {
		let image = build_image(&[(2048, 4095)], 8192);
		struct Failing;
		impl SectorReader for Failing {
			fn read_sector(&self, _lba: u64, _buf: &mut [u8; SECTOR_SIZE]) -> bool {
				false
			}
		}
		assert_eq!(
			read_gpt(&Failing, image.0.len() as u64 / 512),
			Err(GptError::Io)
		);
	}

	#[test]
	fn clips_partition_at_disk_end() {
		let image = build_image(&[(2048, 999_999)], 8192);
		let parts = read_gpt(&image, 8192).expect("valid GPT must parse");
		assert_eq!(parts.len(), 1);
		/* Clipped to the last addressable sector */
		assert_eq!(parts[0].sector_count, 8192 - 2048);
	}

	#[test]
	fn skips_partition_starting_past_disk_end() {
		let image = build_image(&[(2048, 4095), (9000, 9500)], 8192);
		let parts = read_gpt(&image, 8192).expect("valid GPT must parse");
		assert_eq!(parts.len(), 1);
		assert_eq!(parts[0].start_lba, 2048);
	}
}
