#![no_std]

pub mod console;

use limine::framebuffer::Framebuffer;
use limine::memory_map::Entry;

pub unsafe fn write_pixel(ptr: *mut u8, offset: usize, color: &[u8; 4]) {
    unsafe {
        core::ptr::copy_nonoverlapping(color.as_ptr(), ptr.add(offset), 4);
    }
}

pub fn fill_screen_blue(fb: &Framebuffer) {
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let pitch = fb.pitch() as usize;
    let bpp = fb.bpp() as usize;
    let ptr = fb.addr() as *mut u8;
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

pub fn draw_memory_map(fb: &Framebuffer, entries: &[&Entry]) {
    use limine::memory_map::EntryType;

    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let pitch = fb.pitch() as usize;
    let bpp = fb.bpp() as usize;
    let ptr = fb.addr() as *mut u8;

    let count = entries.len();
    let max_count = width.min(count);
    let bar_width = width / max_count.max(1);

    for (i, entry) in entries.iter().take(max_count).enumerate() {
        let color = match entry.entry_type {
            EntryType::USABLE => [0x00, 0xFF, 0x00, 0x00], // green
            EntryType::BOOTLOADER_RECLAIMABLE => [0xFF, 0xFF, 0x00, 0x00], // yellow
            _ => [0x80, 0x80, 0x80, 0x00],                 // gray
        };
        let x_start = i * bar_width;
        let x_end = (x_start + bar_width).min(width);

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
