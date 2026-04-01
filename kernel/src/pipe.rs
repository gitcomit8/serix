/*
 * pipe.rs - Kernel Pipe Implementation
 *
 * Pipes are anonymous, unidirectional byte channels backed by a
 * fixed-size ring buffer. pipe2() creates a (read_end, write_end) fd
 * pair in the calling task's fd table.
 *
 * Blocking behaviour:
 * - Read on empty pipe with open write-end: blocks until data arrives
 * - Read on empty pipe with closed write-end: returns 0 (EOF)
 * - Write to pipe with closed read-end: returns EPIPE
 */

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use vfs::{FileType, INode};

const PIPE_BUFSZ: usize = 4096;

/*
 * struct PipeInner - Shared state between the read and write ends
 */
struct PipeInner {
	buf: [u8; PIPE_BUFSZ],
	read_pos: usize,
	count: usize,
	write_closed: bool,
	read_closed: bool,
	/* Arcs of blocked reader tasks — woken when data is written */
	waiters: Vec<alloc::sync::Arc<spin::Mutex<task::TaskCB>>>,
}

impl PipeInner {
	fn new() -> Self {
		Self {
			buf: [0u8; PIPE_BUFSZ],
			read_pos: 0,
			count: 0,
			write_closed: false,
			read_closed: false,
			waiters: Vec::new(),
		}
	}

	fn write_pos(&self) -> usize {
		(self.read_pos + self.count) % PIPE_BUFSZ
	}

	fn space(&self) -> usize {
		PIPE_BUFSZ - self.count
	}

	/* Copy min(src.len(), space()) bytes into the ring buffer. Returns count written. */
	fn push(&mut self, src: &[u8]) -> usize {
		let n = core::cmp::min(src.len(), self.space());
		for (i, &b) in src[..n].iter().enumerate() {
			self.buf[(self.write_pos() + i) % PIPE_BUFSZ] = b;
		}
		self.count += n;
		n
	}

	/* Copy min(dst.len(), self.count) bytes out of the ring buffer. Returns count read. */
	fn pop(&mut self, dst: &mut [u8]) -> usize {
		let n = core::cmp::min(dst.len(), self.count);
		for (i, d) in dst[..n].iter_mut().enumerate() {
			*d = self.buf[(self.read_pos + i) % PIPE_BUFSZ];
		}
		self.read_pos = (self.read_pos + n) % PIPE_BUFSZ;
		self.count -= n;
		n
	}
}

/*
 * struct PipeReadEnd - Read half of a pipe
 */
pub struct PipeReadEnd(Arc<Mutex<PipeInner>>);

/*
 * struct PipeWriteEnd - Write half of a pipe
 */
pub struct PipeWriteEnd(Arc<Mutex<PipeInner>>);

impl INode for PipeReadEnd {
	fn read(&self, _offset: usize, buf: &mut [u8]) -> usize {
		loop {
			let mut inner = self.0.lock();
			if inner.count > 0 {
				return inner.pop(buf);
			}
			if inner.write_closed {
				return 0; /* EOF */
			}
			/* Block: save our task Arc and yield */
			if let Some(arc) = task::scheduler::current_task_arc() {
				inner.waiters.push(arc);
			}
			drop(inner);
			x86_64::instructions::interrupts::without_interrupts(|| {
				task::block_current_and_switch();
			});
		}
	}

	fn write(&self, _offset: usize, _buf: &[u8]) -> usize { 0 }

	fn metadata(&self) -> FileType { FileType::File }

	fn size(&self) -> usize { self.0.lock().count }
}

impl Drop for PipeReadEnd {
	fn drop(&mut self) {
		self.0.lock().read_closed = true;
	}
}

impl INode for PipeWriteEnd {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }

	fn write(&self, _offset: usize, buf: &[u8]) -> usize {
		let mut inner = self.0.lock();
		if inner.read_closed {
			return usize::MAX; /* EPIPE sentinel */
		}
		let n = inner.push(buf);
		/* Wake any blocked readers */
		let waiters: Vec<_> = inner.waiters.drain(..).collect();
		drop(inner);
		for w in waiters {
			task::scheduler::wake_task(w);
		}
		n
	}

	fn metadata(&self) -> FileType { FileType::File }
}

impl Drop for PipeWriteEnd {
	fn drop(&mut self) {
		self.0.lock().write_closed = true;
		/* Wake blocked readers so they can see EOF */
		let waiters: Vec<_> = self.0.lock().waiters.drain(..).collect();
		for w in waiters {
			task::scheduler::wake_task(w);
		}
	}
}

/*
 * create_pipe - Allocate a new pipe and insert both ends into task's fd table
 * @task_id: Task that owns the pipe fds
 *
 * Return: (read_fd, write_fd) on success
 */
pub fn create_pipe(task_id: u64) -> (u64, u64) {
	use crate::fd::OpenFile;
	let inner = Arc::new(Mutex::new(PipeInner::new()));
	let read_end: Arc<dyn INode> = Arc::new(PipeReadEnd(Arc::clone(&inner)));
	let write_end: Arc<dyn INode> = Arc::new(PipeWriteEnd(inner));

	let read_fd = crate::fd::insert_inode(task_id, read_end);
	let write_fd = crate::fd::insert_inode(task_id, write_end);
	(read_fd, write_fd)
}
