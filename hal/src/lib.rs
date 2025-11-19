/*
 * Hardware Abstraction Layer (HAL)
 *
 * Provides low-level hardware access and initialization including:
 * - CPU control (interrupts, halt)
 * - Port I/O operations
 * - Serial console
 * - CPU topology detection
 */

#![allow(dead_code)]
#![no_std]

pub mod cpu;
pub mod io;
pub mod serial;
pub mod topology;

pub use io::*;
pub use serial::{init_serial, serial_print};
