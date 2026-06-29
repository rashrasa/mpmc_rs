// A better implementation of v3 with some key changes:
// - A VecDeque is used instead of a managed linked list
// - Connections are made through indices instead of pointers
// - Instead of dummy nodes at the ends, connections can also be an enum value (like Front, Back)

mod queue;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use log::debug;

use crate::{
    BlockingReceive, BlockingSend, RecvError, SendError,
    v5::queue::{AtomicQueue, AtomicQueueHandle},
};

#[derive(Debug)]
pub struct Sender<T> {
    handle: AtomicQueueHandle<T>,
    channel: Arc<ChannelInner>,
}

#[derive(Debug)]
pub struct Receiver<T> {
    handle: AtomicQueueHandle<T>,
    channel: Arc<ChannelInner>,
}

#[derive(Debug)]
struct ChannelInner {
    senders: AtomicUsize,
    receivers: AtomicUsize,
}

impl<T: Send> BlockingReceive<T> for Receiver<T> {
    fn recv(&self) -> Result<T, RecvError> {
        match self
            .handle
            .pop_front_wait_with_check(|| self.channel.senders.load(Ordering::SeqCst) > 0)
        {
            Ok(v) => Ok(v),
            Err(e) => match e {
                queue::ReaderError::CheckFailed => Err(RecvError::Closed),
            },
        }
    }
}

impl<T: Send> BlockingSend<T> for Sender<T> {
    fn send(&self, data: T) -> Result<(), SendError<T>> {
        if self.channel.receivers.load(Ordering::SeqCst) == 0 {
            Err(SendError::Closed(data))
        } else {
            self.handle.push(data);

            Ok(())
        }
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
            handle: self.handle.clone(),
            channel: Arc::clone(&self.channel),
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.channel.senders.fetch_add(1, Ordering::SeqCst);

        Self {
            handle: self.handle.clone(),
            channel: Arc::clone(&self.channel),
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let receivers = self.channel.receivers.fetch_sub(1, Ordering::SeqCst);

        self.handle
            .wake_reads(self.channel.receivers.load(Ordering::SeqCst));

        debug!("Receiver count: {}", receivers - 1);
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let senders = self.channel.senders.fetch_sub(1, Ordering::SeqCst);

        self.handle
            .wake_reads(self.channel.receivers.load(Ordering::SeqCst));

        #[cfg(feature = "bench")]
        debug!("Sender count: {}", senders - 1);
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let tx_handle = AtomicQueue::with_capacity(50_000_000);
    let rx_handle = tx_handle.clone();

    let tx_inner = Arc::new(ChannelInner {
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
pub struct V5Maker;
#[cfg(feature = "bench")]
impl crate::BChannelMaker for V5Maker {
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
