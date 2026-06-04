// Extremely naive solution using a Vec, Mutex, and busy-waiting.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{ChannelReceive, ChannelSend, RecvError, SendError};

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

impl<T> ChannelReceive<T> for Receiver<T> {
    fn recv(&self) -> Result<T, RecvError> {
        loop {
            match self.inner.lock() {
                Ok(mut inner) => {
                    if inner.queue.len() > 0 {
                        return Ok(inner.queue.remove(0));
                    } else {
                        // only check for 0 senders if queue is empty
                        if inner.senders < 1 {
                            return Err(RecvError::Closed);
                        }
                    }
                }
                Err(_) => {
                    return Err(RecvError::Closed);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl<T> ChannelSend<T> for Sender<T> {
    fn send(&self, data: T) -> Result<(), SendError<T>> {
        let mut v = match self.inner.lock() {
            Ok(guard) => guard,
            Err(err) => err.into_inner(),
        };

        if v.receivers < 1 {
            return Err(SendError::Closed(data));
        } else {
            v.queue.push(data);
            return Ok(());
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
        self.inner.lock().unwrap().receivers -= 1;
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.inner.lock().unwrap().senders -= 1;
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
