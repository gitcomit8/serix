#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/*
 * IPC Message Constants
 */
pub const MAX_MSG_SIZE: usize = 128;

/*
 * struct Message - Standard IPC message format
 * @sender_id: Sender task ID
 * @id: Message ID/type
 * @len: Message data length
 * @data: Message payload
 *
 * Fits in registers or small stack buffer.
 */
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Message {
	pub sender_id: u64,
	pub id: u64,
	pub len: u64,
	pub data: [u8; MAX_MSG_SIZE],
}

impl Default for Message {
	fn default() -> Self {
		Self {
			sender_id: 0,
			id: 0,
			len: 0,
			data: [0; MAX_MSG_SIZE],
		}
	}
}
