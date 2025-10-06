#![no_std]
extern crate alloc;

use alloc::vec::Vec;

/// Safely write a pixel to the framebuffer at the given offset
pub unsafe fn write_pixel(ptr: *mut u8, offset: usize, color: &[u8; 4]) {
	core::ptr::copy_nonoverlapping(color.as_ptr(), ptr.add(offset), 4);
}

/// Fill the entire screen with blue color
/// Updated to work with Multiboot2 FramebufferTag directly
pub fn fill_screen_blue(framebuffer_tag: &multiboot2::FramebufferTag) {
	let width = framebuffer_tag.width() as usize;
	let height = framebuffer_tag.height() as usize;
	let pitch = framebuffer_tag.pitch() as usize;
	let bpp = framebuffer_tag.bpp() as usize;
	let ptr = framebuffer_tag.address() as *mut u8;

	// Blue pixel in BGRA format (same as original implementation)
	let blue_pixel = [0xFF, 0x00, 0x00, 0x00]; // BGRA

	for y in 0..height {
		for x in 0..width {
			let offset = y * pitch + x * (bpp / 8);
			unsafe {
				write_pixel(ptr, offset, &blue_pixel);
			}
		}
	}
}

/// Draw a memory map visualization at the bottom of the screen
/// Updated to work with Multiboot2 MemoryArea slice
pub fn draw_memory_map(framebuffer_tag: &multiboot2::FramebufferTag, memory_areas: impl Iterator<Item = multiboot2::MemoryArea>) {
	let width = framebuffer_tag.width() as usize;
	let height = framebuffer_tag.height() as usize;
	let pitch = framebuffer_tag.pitch() as usize;
	let bpp = framebuffer_tag.bpp() as usize;
	let ptr = framebuffer_tag.address() as *mut u8;

	let memory_vec: Vec<multiboot2::MemoryArea> = memory_areas.collect();
	let count = memory_vec.len();
	let max_count = width.min(count);
	let bar_width = width / max_count.max(1);

	for (i, area) in memory_vec.iter().take(max_count).enumerate() {
		// Use typ() method to get MemoryAreaTypeId and convert to u32 for matching
		let area_type_id = area.typ().into();
		let color = match area_type_id {
			1 => [0x00, 0xFF, 0x00, 0x00], // Available memory - green
			2 => [0x80, 0x80, 0x80, 0x00], // Reserved memory - gray
			3 => [0xFF, 0xFF, 0x00, 0x00], // ACPI reclaimable - yellow
			4 => [0xFF, 0x00, 0xFF, 0x00], // Reserved hibernation/NVS - magenta
			5 => [0xFF, 0x00, 0x00, 0x00], // Defective memory - red
			_ => [0x80, 0x80, 0x80, 0x00], // Unknown/custom types - gray
		};

		let x_start = i * bar_width;
		let x_end = (x_start + bar_width).min(width);

		// Draw a 40-pixel high bar at the bottom of the screen
		for x in x_start..x_end {
			for y in (height - 40)..height {
				let offset = y * pitch + x * (bpp / 8);
				unsafe {
					write_pixel(ptr, offset, &color);
				}
			}
		}
	}
}

/// Initialize framebuffer (placeholder for future extensions)
/// This function validates the framebuffer is usable
pub fn init_framebuffer(framebuffer_tag: &multiboot2::FramebufferTag) {
	// Basic validation - ensure we have reasonable dimensions
	let width = framebuffer_tag.width();
	let height = framebuffer_tag.height();
	let bpp = framebuffer_tag.bpp();

	// Could add more initialization logic here in the future
	// For now, just ensure we have valid dimensions
	if width == 0 || height == 0 || bpp == 0 {
		// Invalid framebuffer - could panic or handle gracefully
	}
}

/// Get framebuffer information as a tuple (for debugging)
pub fn get_framebuffer_info(framebuffer_tag: &multiboot2::FramebufferTag) -> (usize, usize, usize, usize) {
	(
		framebuffer_tag.width() as usize,
		framebuffer_tag.height() as usize,
		framebuffer_tag.pitch() as usize,
		framebuffer_tag.bpp() as usize,
	)
}

/// Draw a simple rectangular border (utility function)
pub fn draw_border(framebuffer_tag: &multiboot2::FramebufferTag, color: [u8; 4], thickness: usize) {
	let width = framebuffer_tag.width() as usize;
	let height = framebuffer_tag.height() as usize;
	let pitch = framebuffer_tag.pitch() as usize;
	let bpp = framebuffer_tag.bpp() as usize;
	let ptr = framebuffer_tag.address() as *mut u8;

	// Top and bottom borders
	for y in 0..thickness.min(height) {
		for x in 0..width {
			let offset = y * pitch + x * (bpp / 8);
			unsafe {
				write_pixel(ptr, offset, &color);
			}
		}
	}

	for y in (height.saturating_sub(thickness))..height {
		for x in 0..width {
			let offset = y * pitch + x * (bpp / 8);
			unsafe {
				write_pixel(ptr, offset, &color);
			}
		}
	}

	// Left and right borders
	for y in 0..height {
		for x in 0..thickness.min(width) {
			let offset = y * pitch + x * (bpp / 8);
			unsafe {
				write_pixel(ptr, offset, &color);
			}
		}

		for x in (width.saturating_sub(thickness))..width {
			let offset = y * pitch + x * (bpp / 8);
			unsafe {
				write_pixel(ptr, offset, &color);
			}
		}
	}
}
