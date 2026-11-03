/*
 * init/graphics.rs - Graphics & Console Initialization
 *
 * Provides init_graphics() which handles:
 * - Painting screen blue
 * - Drawing memory map visualization
 * - Initializing the framebuffer console (TTY)
 */

use limine::request::FramebufferRequest;
use limine::framebuffer::Framebuffer;
use hal::serial_println;

/* Limine request for framebuffer */
static FB_REQ: FramebufferRequest = FramebufferRequest::new();

/*
 * init_graphics - Initialize framebuffer and console
 * @mmap_entries: Memory map entries from Limine (for visualization)
 *
 * Paints the screen blue, draws the memory map, and initializes
 * the framebuffer console for kernel logging.
 */
pub unsafe fn init_graphics(mmap_entries: &[&limine::memory_map::Entry]) {
	/* Paint screen blue and draw memory map visualization */
	if let Some(fb_response) = FB_REQ.get_response() {
		if let Some(fb) = fb_response.framebuffers().next() {
			graphics::fill_screen_blue(&fb);
			graphics::draw_memory_map(&fb, mmap_entries);
		}
	}

	/* Initialize framebuffer TTY */
	if let Some(fb_response) = FB_REQ.get_response() {
		if let Some(fb) = fb_response.framebuffers().next() {
			graphics::init_console(
				fb.addr(),
				fb.width() as usize,
				fb.height() as usize,
				fb.pitch() as usize,
			);
		}
	}

	serial_println!("");
	graphics::kprintln!("Serix OS v0.0.6");
	graphics::kprintln!("");
	graphics::kprintln!("--- System Check ---");
}
