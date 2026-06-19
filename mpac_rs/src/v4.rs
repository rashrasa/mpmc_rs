// A better implementation of v3 with some key changes:
// - A VecDeque is used instead of a managed linked list
// - Connections are made through indices instead of pointers
// - Instead of dummy nodes at the ends, connections can also be an enum value (like Front, Back)

mod action;
mod queue;
mod status;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    BlockingReceive, BlockingSend, RecvError, SendError,
    v4::queue::{AtomicQueue, ReaderAccessHandle, WriterAccessHandle},
};

#[derive(Debug)]
pub struct Sender<T> {
    handle: WriterAccessHandle<T>,
    channel: Arc<ChannelInner<T>>,
}

#[derive(Debug)]
pub struct Receiver<T> {
    handle: ReaderAccessHandle<T>,
    channel: Arc<ChannelInner<T>>,
}

#[derive(Debug)]
struct ChannelInner<T> {
    queue: Arc<AtomicQueue<T>>,
    senders: AtomicUsize,
    receivers: AtomicUsize,
}

impl<T: Send> BlockingReceive<T> for Receiver<T> {
    fn recv(&self) -> Result<T, RecvError> {
        Ok(self.handle.pop_front_wait())
    }
}

impl<T: Send> BlockingSend<T> for Sender<T> {
    fn send(&self, data: T) -> Result<(), SendError<T>> {
        self.handle.push(data);

        Ok(())
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingReceive<T> for Receiver<T> {
    fn b_recv(&self) -> Result<(T, usize), crate::BRecvError> {
        let len = self.handle.len();
        match self.recv() {
            Ok(v) => Ok((v, len)),
            Err(e) => match e {
                RecvError::Closed => Err(crate::BRecvError::Closed(len)),
            },
        }
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingSend<T> for Sender<T> {
    fn b_send(&self, data: T) -> Result<usize, crate::BSendError<T>> {
        let len = self.handle.len();
        match self.send(data) {
            Ok(_) => Ok(len),
            Err(e) => match e {
                SendError::Closed(v) => Err(crate::BSendError::Closed((v, len))),
            },
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.channel.receivers.fetch_add(1, Ordering::SeqCst);

        Self {
            handle: AtomicQueue::reader(Arc::clone(&self.channel.queue)),
            channel: Arc::clone(&self.channel),
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.channel.senders.fetch_add(1, Ordering::SeqCst);

        Self {
            handle: AtomicQueue::writer(Arc::clone(&self.channel.queue)),
            channel: Arc::clone(&self.channel),
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.channel.receivers.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.channel.senders.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let queue = Arc::new(AtomicQueue::with_capacity(25));
    let tx_handle = AtomicQueue::writer(Arc::clone(&queue));
    let rx_handle = AtomicQueue::reader(Arc::clone(&queue));

    let tx_inner = Arc::new(ChannelInner {
        queue,
        senders: AtomicUsize::new(1),
        receivers: AtomicUsize::new(1),
    });
    let rx_inner = Arc::clone(&tx_inner);

    let tx = Sender {
        handle: tx_handle,
        channel: tx_inner,
    };

    let rx = Receiver {
        handle: rx_handle,
        channel: rx_inner,
    };

    (tx, rx)
}

#[cfg(feature = "bench")]
pub struct V4Maker;
#[cfg(feature = "bench")]
impl crate::BChannelMaker for V4Maker {
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
