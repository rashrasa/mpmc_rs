// SCRAPPED, see v4 instead.

// Currently, all atomic operations use `Ordering::SeqCst`.
// Once correctness is established, a more efficient Ordering will be used for each operation.
// TODO

// TODO: Fix waking mechanism, too much contention around ConcurrentBlockingList.len

mod access_flag;
mod node;

use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use log::{debug, error};

use crate::{
    BlockingReceive, BlockingSend, RecvError, SendError,
    v3::{
        access_flag::{Identity, Status},
        node::Node,
    },
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
            let guard = match self.inner.queue.len.0.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if *guard == 0 {
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
        let len = *match self.inner.queue.len.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
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
        let len = *match self.inner.queue.len.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
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

unsafe impl<T: Send> Send for ConcurrentBlockingList<T> {}
unsafe impl<T: Send> Sync for ConcurrentBlockingList<T> {}

// INVARIANT: front and back are never TAKEN
#[derive(Debug)]
pub struct ConcurrentBlockingList<T> {
    dummy_front: Node<T>,
    dummy_back: Node<T>,

    len: (Mutex<usize>, Condvar),
}

impl<T: Send> ConcurrentBlockingList<T> {
    pub fn new() -> Self {
        let dummy_front = Node::new_front();
        let dummy_back = Node::new_back();

        // SAFETY: We have exclusive access to both
        unsafe {
            dummy_back.set_next(&dummy_front);
            dummy_front.set_next(&dummy_back);
        }

        Self {
            dummy_front,
            dummy_back,

            len: (Mutex::new(0), Condvar::new()),
        }
    }

    pub fn pop_front_wait(&self) -> T {
        // wait for a push if necessary
        let (lock, cvar) = &self.len;
        let mut len_guard = match lock.lock() {
            Ok(g) => g,
            Err(p) => {
                error!("poison error: {:?}", p);
                p.into_inner()
            }
        };
        let mut len = *len_guard;
        while len == 0 {
            len_guard = cvar.wait(len_guard).unwrap();
            len = *len_guard;
        }
        *len_guard -= 1;
        drop(len_guard);

        // at this point, there should be an element for this
        // receiver to take

        // Mark front as accessed
        // Could be contending with another receiver if len > 1
        let dummy_front = &self.dummy_front;
        let dummy_front_guard = loop {
            // SAFETY: Always safe to try_access dummy nodes
            if let Ok(g) = unsafe { dummy_front.try_access() } {
                break g;
            }
        };

        // SAFETY: We have access to dummy front. No other receiver will get through.
        // A sender may be in the middle of updating its next pointer
        // but senders never take.
        let front = unsafe { self.dummy_front.next_node() }.unwrap();

        // SAFETY: We have exclusive access to front
        let front_ident = unsafe { front.identity() };
        if front_ident == Identity::Back {
            unreachable!(
                "receiver attempting to take without an element present. queue len: {}",
                len
            );
        }

        // Declare desire to take the real front node
        // Could be contending with a sender pushing an element here.
        loop {
            // SAFETY: We may have shared access to front with a
            //         sender, who will never take.
            if let Ok(g) = unsafe { front.try_declare_take() } {
                break g;
            }
        }

        // SAFETY
        // - If it's the dummy back node, it's never TAKEN
        // - If it's another node:
        //   - A sender may have access to it to update its next pointer.
        //     In this case, we just have to wait until we can update the access flag to ACCESSED.
        //   - A receiver can't be in the process of taking it since we guard the front dummy node with ACCESSED.
        let front_next = unsafe { front.next_node() }.unwrap();

        // DEADLOCK: len == 1, recv holds dummy_front + front, send holds dummy_back
        // recv waits for dummy_back, send waits for front.

        // INSIGHT: We actually don't need access to front_next if it == dummy_back. It
        // will never be TAKEN. If a push is in progress, it will wait for front.
        // After we pop front, sender will just access dummy_front instead of front, then push the new node.
        // We just have to make sure that at this stage, push_back gives up access to dummy_back.

        // We must acquire this guard. At this point,
        // we could be waiting for a sender to realize we've declared
        // the intention to take and to then release this node.
        let front_next_guard = loop {
            // SAFETY: We may have shared access to front with a
            //         sender, who will never take.
            if let Ok(g) = unsafe { front_next.try_access() } {
                break g;
            }
        };

        // now we have taken next. We can update front to be next

        // SAFETY: We have exclusive access to dummy_front and front_next
        unsafe {
            dummy_front.set_next(front_next);
        }

        // If front_next == dummy_back, it expects to point to this item (also means len == 1 here)

        // SAFETY: we have exclusive access
        if unsafe { front_next.identity() } == Identity::Back {
            unsafe {
                front_next.set_next(dummy_front);
            }
        }

        drop(front_next_guard);
        // front_next is no longer valid

        drop(dummy_front_guard);
        // dummy_front is no longer valid

        if len > 0 {
            // only one receiver at a time, so we must wake others
            self.len.1.notify_one();
        }

        // we now have ownership of front

        // At this point, we detached the front node from the list and
        // released access to all resources.

        // SAFETY: We have exclusive access to front and it does not get dereferenced after the swap_take_drop call
        unsafe { front.swap_take_drop() }
    }

    pub fn push_back(&self, data: T) {
        // Deadlock prevention: Sender keeps releasing dummy_back if receiver is trying to pop.
        // Only relevant for len == 1
        let (dummy_back, dummy_back_guard, back, back_guard) = loop {
            let dummy_back = &self.dummy_back;
            let dummy_back_guard = loop {
                // SAFETY: Always safe to access a dummy node
                if let Ok(g) = unsafe { dummy_back.try_access() } {
                    break g;
                }
            };

            // SAFETY: we have access to dummy_back
            let back = unsafe { dummy_back.next_node() }.unwrap();
            // this could be the front_dummy, a node. all irrelevant

            let back_guard = loop {
                // SAFETY: If this was a node being taken,
                // we wouldn't have access to dummy_back at this point. We do,
                // which means a pop is not in progress.
                match unsafe { back.try_access() } {
                    Ok(g) => break Ok(g),
                    Err(Status::DeclareTake) => {
                        // release dummy_back_guard
                        break Err(());
                    }
                    Err(_) => {}
                }
            };
            if let Ok(bg) = back_guard {
                break (dummy_back, dummy_back_guard, back, bg);
            }
        };
        let node = Box::leak(Box::new(Node::new_node(data)));

        // SAFETY: We have exclusive access to node and dummy_back
        unsafe {
            node.set_next(dummy_back);
        }

        // SAFETY: We have exclusive access to dummy_back and node.
        unsafe {
            dummy_back.set_next(node);
        }

        // SAFETY: We have exclusive access to back and node.
        unsafe {
            back.set_next(node);
        }

        drop(back_guard);
        // back is no longer valid

        drop(dummy_back_guard);
        // dummy_back is no longer valid

        // wake someone
        let mut len_guard = match self.len.0.lock() {
            Ok(g) => g,
            Err(p) => {
                error!("poison error: {:?}", p);
                p.into_inner()
            }
        };
        *len_guard += 1;
        let len = *len_guard;
        drop(len_guard);
        if len == 1 {
            // Notify if the len went from 0 to 1
            self.len.1.notify_one();
        }
    }
}

impl<T: Send> Default for ConcurrentBlockingList<T> {
    fn default() -> Self {
        let dummy_front = Node::new_front();
        let dummy_back = Node::new_back();

        // SAFETY: We have exclusive access to both
        unsafe {
            dummy_back.set_next(&dummy_front);
            dummy_front.set_next(&dummy_back);
        }

        Self {
            dummy_front,
            dummy_back,

            len: (Mutex::new(0), Condvar::new()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Instant};

    #[test]
    fn empty_structure_valid() {
        let queue = ConcurrentBlockingList::<u8>::new();

        let dummy_front = &queue.dummy_front;
        assert_eq!(unsafe { dummy_front.identity() }, Identity::Front);

        let dummy_back = unsafe { dummy_front.next_node() }.unwrap();
        assert_eq!(unsafe { dummy_back.identity() }, Identity::Back);

        let dummy_back_list = &queue.dummy_back;
        assert_eq!(unsafe { dummy_back_list.identity() }, Identity::Back);
    }

    #[test]
    fn push_back_structure_valid() {
        let queue = ConcurrentBlockingList::<u8>::new();

        let v = 5;
        queue.push_back(v);

        let dummy_front = &queue.dummy_front;
        assert_eq!(unsafe { dummy_front.identity() }, Identity::Front);

        let front = unsafe { dummy_front.next_node() }.unwrap();
        assert_eq!(unsafe { front.identity() }, Identity::Node);
        assert_eq!(*unsafe { front.read_inner() }, v);

        let dummy_back = unsafe { front.next_node() }.unwrap();
        assert_eq!(unsafe { dummy_back.identity() }, Identity::Back);

        let dummy_back_list = &queue.dummy_back;
        assert_eq!(unsafe { dummy_back_list.identity() }, Identity::Back);
    }

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
