/*
 * Task Waker
 *
 * Provides a dummy waker implementation for the executor.
 * The waker does nothing since our executor polls all tasks round-robin.
 */

use core::task::{RawWaker, RawWakerVTable, Waker};

/* No-op function for waker vtable */
fn no_op(_: *const ()) {}

/* Clone function for waker */
fn clone_waker(_: *const ()) -> RawWaker {
	raw_waker()
}

/* Create a raw waker with no-op vtable */
fn raw_waker() -> RawWaker {
	RawWaker::new(core::ptr::null(), &VTABLE)
}

/* Virtual function table with no-op implementations */
const VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, no_op, no_op, no_op);

/*
 * dummy_waker - Create a dummy waker
 *
 * Returns a waker that does nothing when woken.
 */
pub fn dummy_waker() -> Waker {
	unsafe { Waker::from_raw(raw_waker()) }
}
