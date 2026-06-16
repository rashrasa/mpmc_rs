// Exactly the same as v1, except it uses a VecDeque

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use log::{debug, error};

use crate::{BlockingReceive, BlockingSend, RecvError, SendError};

#[derive(Debug)]
pub struct Sender<T> {
    inner: Arc<Mutex<ChannelInner<T>>>,
}

#[derive(Debug)]
pub struct Receiver<T> {
    inner: Arc<Mutex<ChannelInner<T>>>,
}

#[derive(Debug)]
struct ChannelInner<T> {
    senders: usize,
    receivers: usize,
    queue: VecDeque<T>,
}

impl<T: Send> BlockingReceive<T> for Receiver<T> {
    fn recv(&self) -> Result<T, RecvError> {
        loop {
            {
                let mut inner = match self.inner.lock() {
                    Ok(g) => g,
                    Err(err) => {
                        error!("Poison Error: {:?}", err);
                        err.into_inner()
                    }
                };
                let queue_len = inner.queue.len();
                if queue_len > 0 {
                    return Ok(inner.queue.pop_front().unwrap());
                } else {
                    // only check for 0 senders if queue is empty
                    if inner.senders == 0 {
                        return Err(RecvError::Closed);
                    }
                    // Senders are still active but no messages are in the queue.
                }
            }
        }
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingReceive<T> for Receiver<T> {
    fn b_recv(&self) -> Result<(T, usize), crate::BRecvError> {
        loop {
            {
                let mut inner = match self.inner.lock() {
                    Ok(g) => g,
                    Err(err) => {
                        error!("Poison Error: {:?}", err);
                        err.into_inner()
                    }
                };
                let queue_len = inner.queue.len();
                if queue_len > 0 {
                    return Ok((inner.queue.pop_front().unwrap(), queue_len));
                } else {
                    // only check for 0 senders if queue is empty
                    if inner.senders == 0 {
                        return Err(crate::BRecvError::Closed(queue_len));
                    }
                    // Senders are still active but no messages are in the queue.
                }
            }
        }
    }
}

impl<T: Send> BlockingSend<T> for Sender<T> {
    fn send(&self, data: T) -> Result<(), SendError<T>> {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Poison Error: {:?}", err);
                err.into_inner()
            }
        };

        if inner.receivers == 0 {
            return Err(SendError::Closed(data));
        } else {
            inner.queue.push_back(data);
            return Ok(());
        }
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingSend<T> for Sender<T> {
    fn b_send(&self, data: T) -> Result<usize, crate::BSendError<T>> {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Poison Error: {:?}", err);
                err.into_inner()
            }
        };

        if inner.receivers == 0 {
            return Err(crate::BSendError::Closed((data, inner.queue.len())));
        } else {
            inner.queue.push_back(data);
            return Ok(inner.queue.len());
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        let rx = self.inner.clone();
        rx.lock().unwrap().receivers += 1;
        Self { inner: rx }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let tx = self.inner.clone();
        tx.lock().unwrap().senders += 1;
        Self { inner: tx }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut locked = self.inner.lock().unwrap();

        #[cfg(feature = "bench")]
        debug!("Receiver count: {}", locked.receivers - 1);

        locked.receivers -= 1;
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut locked = self.inner.lock().unwrap();

        #[cfg(feature = "bench")]
        debug!("Sender count: {}", locked.senders - 1);

        locked.senders -= 1;
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(ChannelInner {
        senders: 1,
        receivers: 1,
        queue: VecDeque::new(),
    }));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}

#[cfg(feature = "bench")]
pub struct V2Maker;
#[cfg(feature = "bench")]
impl crate::BChannelMaker for V2Maker {
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
