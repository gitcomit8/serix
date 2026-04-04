/*
 * fmt.rs - Formatted output macros for userspace
 *
 * Implements core::fmt::Write over the write() syscall so that
 * print! and println! work in no_std userspace binaries.
 */

use core::fmt::{self, Write};
use crate::{STDOUT, write as sys_write};

struct StdoutWriter;

impl Write for StdoutWriter {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		sys_write(STDOUT, s.as_bytes());
		Ok(())
	}
}

/* _print - Write formatted arguments to stdout (called by macros) */
pub fn _print(args: fmt::Arguments) {
	StdoutWriter.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
	($($arg:tt)*) => {
		$crate::fmt::_print(::core::format_args!($($arg)*))
	};
}

#[macro_export]
macro_rules! println {
	() => { $crate::print!("\n") };
	($($arg:tt)*) => {
		$crate::fmt::_print(::core::format_args!("{}\n", ::core::format_args!($($arg)*)))
	};
}
