/*
 * ext4/ipc.rs - IPC protocol constants shared by kernel stub and daemon
 *
 * Port topology:
 *   EXT4_REQ_PORT   - daemon listens here for requests
 *   EXT4_REPLY_BASE - kernel listens here for replies (sender_id offset)
 *
 * Message IDs:
 *   MSG_LOOKUP   - look up a name in a directory
 *   MSG_STAT     - get metadata for an inode
 *   MSG_READ     - read data from a file
 *   MSG_WRITE    - write data to a file
 *   MSG_READDIR  - list directory entries
 *   MSG_MKDIR    - create a subdirectory
 *   MSG_CREATE   - create a regular file
 *   MSG_UNLINK   - delete a directory entry
 *   MSG_SIZE     - get file size
 */

pub const EXT4_REQ_PORT:   u64 = 0x0000_4E00;
pub const EXT4_REPLY_BASE: u64 = 0x0000_4E01;
pub const MSG_LOOKUP:   u64 = 1;
pub const MSG_STAT:     u64 = 2;
pub const MSG_READ:     u64 = 3;
pub const MSG_WRITE:    u64 = 4;
pub const MSG_READDIR:  u64 = 5;
pub const MSG_MKDIR:    u64 = 6;
pub const MSG_CREATE:   u64 = 7;
pub const MSG_UNLINK:   u64 = 8;
pub const MSG_SIZE:     u64 = 9;
pub const MAX_DATA: usize = 112;
