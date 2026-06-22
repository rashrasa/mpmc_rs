// SCRAPPED, see v4 instead.

// Currently, all atomic operations use `Ordering::SeqCst`.
// Once correctness is established, a more efficient Ordering will be used for each operation.
// TODO

// TODO: Fix waking mechanism, too much contention around ConcurrentBlockingList.len

mod access_flag;
mod node;
mod queue;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use log::debug;

use crate::{
    BlockingReceive, BlockingSend, RecvError, SendError, v3::queue::ConcurrentBlockingList,
};

#[derive(Debug)]
pub struct Sender<T> {
    inner: Arc<ChannelInner<T>>,
}

#[derive(Debug)]
pub struct Receiver<T> {
    inner: Arc<ChannelInner<T>>,
}

#[derive(Debug)]
struct ChannelInner<T> {
    senders: AtomicUsize,
    receivers: AtomicUsize,
    queue: ConcurrentBlockingList<T>,
}

impl<T: Send> BlockingReceive<T> for Receiver<T> {
    fn recv(&self) -> Result<T, RecvError> {
        let n_send = self.inner.senders.load(Ordering::SeqCst);
        if n_send == 0 {
            let len = self.inner.queue.len();
            if len == 0 {
                return Err(RecvError::Closed);
            }
        }

        Ok(self.inner.queue.pop_front_wait())
    }
}

impl<T: Send> BlockingSend<T> for Sender<T> {
    fn send(&self, data: T) -> Result<(), SendError<T>> {
        let n_recv = self.inner.receivers.load(Ordering::SeqCst);
        if n_recv == 0 {
            Err(SendError::Closed(data))
        } else {
            self.inner.queue.push_back(data);
            Ok(())
        }
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingReceive<T> for Receiver<T> {
    fn b_recv(&self) -> Result<(T, usize), crate::BRecvError> {
        let n_send = self.inner.senders.load(Ordering::SeqCst);
        let len = self.inner.queue.len();

        if n_send == 0 && len == 0 {
            Err(crate::BRecvError::Closed(len))
        } else {
            Ok((self.inner.queue.pop_front_wait(), len))
        }
    }
}

#[cfg(feature = "bench")]
impl<T: Send> crate::BBlockingSend<T> for Sender<T> {
    fn b_send(&self, data: T) -> Result<usize, crate::BSendError<T>> {
        let n_recv = self.inner.receivers.load(Ordering::SeqCst);
        let len = self.inner.queue.len();
        if n_recv == 0 {
            Err(crate::BSendError::Closed((data, len)))
        } else {
            self.inner.queue.push_back(data);
            Ok(len)
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.inner.receivers.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.inner.senders.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let n_receivers = self.inner.receivers.fetch_sub(1, Ordering::SeqCst);

        #[cfg(feature = "bench")]
        debug!("Receiver count: {}", n_receivers - 1);
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let n_receivers = self.inner.senders.fetch_sub(1, Ordering::SeqCst);

        #[cfg(feature = "bench")]
        debug!("Sender count: {}", n_receivers - 1);
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>)
where
    T: Send,
{
    let inner = Arc::new(ChannelInner {
        senders: AtomicUsize::new(1),
        receivers: AtomicUsize::new(1),
        queue: ConcurrentBlockingList::new(),
    });
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Instant};

    #[test]
    fn queue_works() {
        let msg = 5;

        let (tx, rx) = channel::<i32>();

        let t0 = thread::spawn(move || {
            tx.send(msg).unwrap();
        });

        let t1 = thread::spawn(move || rx.recv().unwrap());
        t0.join().unwrap();

        let result = t1.join().unwrap();

        assert_eq!(msg, result);
    }

    #[test]
    fn n_messages_aggregated() {
        let n = 5000;
        let msg = 5;

        let (tx, rx) = channel::<i32>();

        let t0 = thread::spawn(move || {
            for _ in 0..n {
                tx.send(msg).unwrap();
            }
        });

        let t1 = thread::spawn(move || {
            let mut count = 0;

            while let Ok(v) = rx.recv() {
                count += v;
            }
            count
        });
        t0.join().unwrap();

        let result = t1.join().unwrap();

        assert_eq!(n * msg, result);
    }

    #[test]
    fn n_seconds_aggregated() {
        let start = Instant::now();
        let seconds = 3.0;
        let msg = 5.0;

        let (tx, rx) = channel();

        let t0 = thread::spawn(move || {
            let mut sent = 0.0;
            while Instant::now().duration_since(start).as_secs_f64() < seconds {
                tx.send(msg).unwrap();
                sent += msg;
            }
            sent
        });

        let t1 = thread::spawn(move || {
            let mut received = 0.0;

            while let Ok(v) = rx.recv() {
                received += v;
            }
            received
        });

        let sent = t0.join().unwrap();
        let received = t1.join().unwrap();

        assert_eq!(sent, received);
    }
}
