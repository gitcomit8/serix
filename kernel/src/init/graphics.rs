/*
 * init/graphics.rs - Graphics & Console Initialization
 *
 * Provides init_graphics() which handles:
 * - Painting screen blue
 * - Drawing memory map visualization
 * - Initializing the framebuffer console (TTY)
 */

use hal::serial_println;

/*
 * init_graphics - Initialize framebuffer and console
 * @boot_info: BootInfo from bootloader (validated)
 *
 * Paints the screen blue, draws the memory map, and initializes
 * the framebuffer console for kernel logging.
 */
pub unsafe fn init_graphics(boot_info: &'static boot::BootInfo) {
	/* Paint screen blue and draw memory map visualization */
	if let Some(fb) = boot_info.framebuffer {
		graphics::fill_screen_blue(fb);
		graphics::draw_memory_map(fb, boot_info.memory_regions);
	}

	/* Initialize framebuffer TTY */
	if let Some(fb) = boot_info.framebuffer {
		graphics::init_console(
			fb.address as *mut u8,
			fb.width as usize,
			fb.height as usize,
			fb.pitch as usize,
		);
	}

	serial_println!("");
	graphics::kprintln!("Serix OS v0.0.6");
	graphics::kprintln!("");
	graphics::kprintln!("--- System Check ---");
}
