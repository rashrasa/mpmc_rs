// Extremely naive solution using a Vec, Mutex, and busy-waiting.

use std::sync::{Arc, Mutex};

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
    queue: Vec<T>,
}

impl<T> BlockingReceive<T> for Receiver<T> {
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
                    return Ok(inner.queue.remove(0));
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

    #[cfg(feature = "bench")]
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
                    return Ok((inner.queue.remove(0), queue_len));
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

impl<T> BlockingSend<T> for Sender<T> {
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
            inner.queue.push(data);
            return Ok(());
        }
    }

    #[cfg(feature = "bench")]
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
            inner.queue.push(data);
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
        queue: vec![],
    }));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}

#[cfg(feature = "bench")]
pub struct V1Maker;
#[cfg(feature = "bench")]
impl crate::ChannelMaker for V1Maker {
    fn channel<T>(&self) -> (Sender<T>, Receiver<T>) {
        channel()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let (tx, rx) = channel::<usize>();
        let tx_handles = (0..20).map(move |_| {
            let tx_thread = tx.clone();
            std::thread::spawn(move || {
                let tx = tx_thread;
                tx.send(1000).unwrap();
            })
        });

        let rx_handles = (0..10).map(move |_| {
            let rx_thread = rx.clone();
            std::thread::spawn(move || {
                let rx = rx_thread;
                let mut counter = 0;
                while let Ok(v) = rx.recv() {
                    counter += v;
                }

                counter
            })
        });

        for handle in tx_handles {
            handle.join().unwrap();
        }

        let mut count = 0;
        for handle in rx_handles {
            count += handle.join().unwrap();
        }

        assert_eq!(count, 20_000);
    }
}
