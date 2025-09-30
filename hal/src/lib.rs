#![allow(dead_code)]
#![no_std]

pub mod cpu;
pub mod io;
pub mod serial;
pub use io::*;
pub use serial::{init_serial, serial_print};
