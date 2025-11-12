use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use alloc::boxed::Box;

pub struct AsyncTask {
    future: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    waker: Option<Waker>,
}

impl AsyncTask {
    pub fn new<F>(future: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            future: Box::pin(future),
            waker: None,
        }
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let result = self.future.as_mut().poll(cx);
        if let Poll::Pending = result {
            self.waker = Some(cx.waker().clone());
        }
        result
    }

    pub fn wake(&self) {
        if let Some(waker) = &self.waker {
            waker.wake_by_ref();
        }
    }
}
