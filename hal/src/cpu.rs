use x86_64::instructions::{hlt, interrupts};
#[inline(always)]
pub fn halt() {
	hlt();
}

#[inline(always)]
pub fn enable_interrupts() {
	interrupts::enable();
}

#[inline(always)]
pub fn disable_interrupts() {
	interrupts::disable();
}
