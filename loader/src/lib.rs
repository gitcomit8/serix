#![no_std]
extern crate alloc;

pub mod elf;

use alloc::vec::Vec;
use elf::{Elf64Header, ProgramHeader, SegmentType, PF_R, PF_W, PF_X};
use x86_64::VirtAddr;

#[derive(Debug)]
pub struct LoadableSegment {
	pub virtual_address: VirtAddr,
	pub size: u64,
	pub flags: SegmentFlags,
	pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct SegmentFlags {
	pub readable: bool,
	pub writable: bool,
	pub executable: bool,
}

#[derive(Debug)]
pub struct LoadedImage {
	pub entry_point: VirtAddr,
	pub segments: Vec<LoadableSegment>,
}

pub fn load_elf(data: &[u8]) -> Result<LoadedImage, &'static str> {
	// 1. Safety check: ensure data is large enough for header
	if data.len() < core::mem::size_of::<Elf64Header>() {
		return Err("File too small");
	}

	// 2. Transmute raw bytes to ELF Header struct
	let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };
	header.validate()?;

	// 3. Parse Program Headers
	let ph_offset = header.e_phoff as usize;
	let ph_count = header.e_phnum as usize;
	let ph_size = header.e_phentsize as usize;

	if data.len() < ph_offset + (ph_count * ph_size) {
		return Err("Program headers truncated");
	}

	let mut segments = Vec::new();

	for i in 0..ph_count {
		let ptr = unsafe { data.as_ptr().add(ph_offset + i * ph_size) };
		let ph = unsafe { &*(ptr as *const ProgramHeader) };

		// We only care about LOAD segments
		if ph.p_type == SegmentType::Load as u32 {
			// Check bounds
			if ph.p_offset + ph.p_filesz > data.len() as u64 {
				return Err("Segment truncated");
			}

			// Prepare data
			let mut segment_data = Vec::with_capacity(ph.p_memsz as usize);

			// Copy file data
			let start = ph.p_offset as usize;
			let end = start + ph.p_filesz as usize;
			segment_data.extend_from_slice(&data[start..end]);

			// Zero-fill .bss section (memory size > file size)
			let zero_fill = (ph.p_memsz - ph.p_filesz) as usize;
			segment_data.resize(segment_data.len() + zero_fill, 0);

			segments.push(LoadableSegment {
				virtual_address: VirtAddr::new(ph.p_vaddr),
				size: ph.p_memsz,
				flags: SegmentFlags {
					readable: ph.p_flags & PF_R != 0,
					writable: ph.p_flags & PF_W != 0,
					executable: ph.p_flags & PF_X != 0,
				},
				data: segment_data,
			});
		}
	}

	Ok(LoadedImage {
		entry_point: VirtAddr::new(header.e_entry),
		segments,
	})
}
