/*
 * init/graphics.rs - Graphics & Console Initialization
 *
 * Provides init_graphics() which initializes the framebuffer TTY.
 */

use hal::serial_println;
use limine::request::FramebufferRequest;

/* Limine request for framebuffer */
static FB_REQ: FramebufferRequest = FramebufferRequest::new();

/*
 * init_graphics - Initialize framebuffer and console
 * @_mmap_entries: Retained for compatibility with the existing boot sequence.
 *
 * The Limine response is fetched once and reused for console setup.
 * Only 32-bit-per-pixel framebuffers with valid geometry are supported.
 * An unsupported or invalid framebuffer leaves the framebuffer console
 * disabled while serial logging remains available.
 */
pub unsafe fn init_graphics(_mmap_entries: &[&limine::memory_map::Entry]) {
	let fb_response = match FB_REQ.get_response() {
		Some(response) => response,
		None => {
			serial_println!("[GRAPHICS] no framebuffer response; console unavailable");
			return;
		}
	};

	let fb = match fb_response.framebuffers().next() {
		Some(fb) => fb,
		None => {
			serial_println!("[GRAPHICS] no framebuffer provided; console unavailable");
			return;
		}
	};

	if fb.bpp() != 32 {
		serial_println!(
			"[GRAPHICS] unsupported framebuffer format: {} bpp (32 required); console unavailable",
			fb.bpp()
		);
		return;
	}

	/* Initialize framebuffer TTY */
	if !graphics::init_console(
		fb.addr(),
		fb.width() as usize,
		fb.height() as usize,
		fb.pitch() as usize,
		fb.bpp() as usize,
	) {
		serial_println!("[GRAPHICS] invalid framebuffer geometry; console unavailable");
		return;
	}

	serial_println!("");
	graphics::kprintln!("Serix OS v{}", crate::VERSION);
	graphics::kprintln!("");
	graphics::kprintln!("--- System Check ---");
}
