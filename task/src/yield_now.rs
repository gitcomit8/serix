/*
 * Cooperative Task Yielding
 *
 * Implements async yield primitive for cooperative multitasking.
 */

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

/*
 * struct YieldNow - Future that yields once
 * @yielded: Flag tracking if we've yielded already
 */
pub struct YieldNow {
	yielded: bool,
}

impl YieldNow {
	pub fn new() -> Self {
		Self { yielded: false }
	}
}

impl Future for YieldNow {
	type Output = ();

	/*
	 * poll - Poll the yield future
	 *
	 * Returns Pending once to yield, then Ready to complete.
	 */
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
		if self.yielded {
			Poll::Ready(())
		} else {
			self.yielded = true;
			cx.waker().wake_by_ref();
			Poll::Pending
		}
	}
}

/*
 * yield_now - Async function to yield control
 *
 * Allows other tasks to run before resuming.
 */
pub async fn yield_now() {
	YieldNow::new().await
}
