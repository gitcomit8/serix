/*
 * Async Task Wrapper
 *
 * Wraps Rust futures for use in the task executor.
 */

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use alloc::boxed::Box;

/*
 * struct AsyncTask - Wrapper for async futures
 * @future: The boxed future being executed
 * @waker: Optional waker for task notification
 */
pub struct AsyncTask {
	future: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
	waker: Option<Waker>,
}

impl AsyncTask {
	/*
	 * new - Create a new async task from a future
	 * @future: Future to wrap
	 */
	pub fn new<F>(future: F) -> Self
	where
		F: Future<Output = ()> + Send + 'static,
	{
		Self {
			future: Box::pin(future),
			waker: None,
		}
	}

	/*
	 * poll - Poll the future
	 * @cx: Task context containing waker
	 *
	 * Returns Poll::Ready when complete, Poll::Pending if still running.
	 */
	pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
		let result = self.future.as_mut().poll(cx);
		if let Poll::Pending = result {
			self.waker = Some(cx.waker().clone());
		}
		result
	}

	/*
	 * wake - Wake the task if it has a waker
	 */
	pub fn wake(&self) {
		if let Some(waker) = &self.waker {
			waker.wake_by_ref();
		}
	}
}
