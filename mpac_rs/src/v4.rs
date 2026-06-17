// A better implementation of v3 with some key changes:
// - A VecDeque is used instead of a managed linked list
// - Connections are made through indices instead of pointers
// - Instead of dummy nodes at the ends, connections can also be an enum value (like Front, Back)

mod queue;

use std::sync::Arc;

use crate::{BlockingReceive, BlockingSend, RecvError, SendError, v4::queue::AtomicQueue};

#[derive(Debug)]
pub struct Sender<T> {
    inner: Arc<AtomicQueue<T>>,
}

#[derive(Debug)]
pub struct Receiver<T> {
    inner: Arc<AtomicQueue<T>>,
}

struct ChannelInner<T> {
    data: AtomicQueue<T>,
}

impl<T: Send> BlockingReceive<T> for Receiver<T> {
    fn recv(&self) -> Result<T, RecvError> {
        todo!()
    }
}

impl<T: Send> BlockingSend<T> for Sender<T> {
    fn send(&self, data: T) -> Result<(), SendError<T>> {
        todo!()
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingReceive<T> for Receiver<T> {
    fn b_recv(&self) -> Result<(T, usize), crate::BRecvError> {
        todo!()
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingSend<T> for Sender<T> {
    fn b_send(&self, data: T) -> Result<usize, crate::BSendError<T>> {
        todo!()
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        todo!()
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        todo!()
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    todo!()
}

#[cfg(feature = "bench")]
pub struct V3Maker;
#[cfg(feature = "bench")]
impl crate::BChannelMaker for V3Maker {
    fn channel<T>(
        &self,
    ) -> (
        impl crate::BBlockingSend<T> + Send + Clone + 'static,
        impl crate::BBlockingReceive<T> + Send + Clone + 'static,
    )
    where
        T: Send + 'static,
    {
        channel()
    }
}
