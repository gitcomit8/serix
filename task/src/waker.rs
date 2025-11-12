use core::task::{RawWaker, RawWakerVTable, Waker};

fn no_op(_: *const ()) {}

fn clone_waker(_: *const ()) -> RawWaker {
    raw_waker()
}

fn raw_waker() -> RawWaker {
    RawWaker::new(core::ptr::null(), &VTABLE)
}

const VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, no_op, no_op, no_op);

pub fn dummy_waker() -> Waker {
    unsafe { Waker::from_raw(raw_waker()) }
}
