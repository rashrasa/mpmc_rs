// Currently, all atomic operations use `Ordering::SeqCst`.
// Once correctness is established, a more efficient Ordering will be used for each operation.
// TODO

mod access_flag;
mod node;

use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use log::error;

use crate::{
    BlockingReceive, BlockingSend, RecvError, SendError,
    v3::{access_flag::Identity, node::Node},
};

#[derive(Debug)]
pub struct Sender<T: 'static + Send> {
    inner: Arc<ChannelInner<T>>,
}

#[derive(Debug)]
pub struct Receiver<T: 'static + Send> {
    inner: Arc<ChannelInner<T>>,
}

#[derive(Debug)]
struct ChannelInner<T: 'static + Send> {
    senders: AtomicUsize,
    receivers: AtomicUsize,
    queue: ConcurrentBlockingList<T>,
}

impl<T: Send + 'static> BlockingReceive<T> for Receiver<T> {
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

impl<T: Send + 'static> BlockingSend<T> for Sender<T> {
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
impl<T: Send + 'static> crate::BBlockingReceive<T> for Receiver<T> {
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
impl<T: Send + 'static> crate::BBlockingSend<T> for Sender<T> {
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

impl<T: 'static + Send> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.inner.receivers.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: 'static + Send> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.inner.senders.fetch_add(1, Ordering::SeqCst);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: 'static + Send> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.inner.receivers.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<T: 'static + Send> Drop for Sender<T> {
    fn drop(&mut self) {
        self.inner.senders.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>)
where
    T: 'static + Send,
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
impl crate::ChannelMaker for V3Maker {
    fn channel<T>(
        &self,
    ) -> (
        impl crate::BBlockingSend<T> + Send + 'static,
        impl crate::BBlockingReceive<T> + Send + 'static,
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
pub struct ConcurrentBlockingList<T: 'static + Send> {
    dummy_front: &'static Node<T>,
    dummy_back: &'static Node<T>,

    len: (Mutex<usize>, Condvar),
}

impl<T: 'static + Send> ConcurrentBlockingList<T> {
    pub fn new() -> Self {
        let dummy_front = Node::new_leaked_front();
        let dummy_back = Node::new_leaked_back();

        // SAFETY: We have exclusive access to both
        unsafe {
            dummy_back.set_next(dummy_front);
            dummy_front.set_next(dummy_back);
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
        let mut len = match lock.lock() {
            Ok(g) => g,
            Err(p) => {
                error!("poison error: {:?}", p);
                p.into_inner()
            }
        };
        while *len == 0 {
            len = cvar.wait(len).unwrap();
        }
        drop(len);

        // at this point, there should be an element for this
        // receiver to take

        // Mark front as accessed
        // Could be contending with another receiver if len > 1
        let dummy_front = self.dummy_front;
        let dummy_front_guard = loop {
            // SAFETY: Always safe to try_access dummy nodes
            match unsafe { dummy_front.try_access() } {
                Ok(g) => break g,
                Err(_) => {}
            }
        };

        // SAFETY: We have access to dummy front. No other receiver will get through.
        // A sender may be in the middle of updating its next pointer
        // but senders never take.
        let front = unsafe { self.dummy_front.next_node() };

        // SAFETY: We have exclusive access to front
        let front_ident = unsafe { front.identity() };
        if front_ident == Identity::Back {
            unreachable!("receiver attempting to take without an element present");
        }

        // Access the real front node
        // Could be contending with a sender pushing an element here.
        let front_guard = loop {
            // SAFETY: We may have shared access to front with a
            //         sender, who will never take.
            match unsafe { front.try_access() } {
                Ok(g) => break g,
                Err(_) => {}
            }
        };

        // SAFETY
        // - If it's the dummy back node, it's never TAKEN
        // - If it's another node:
        //   - A sender may have access to it to update its next pointer.
        //     In this case, we just have to wait until we can update the access flag to ACCESSED.
        //   - A receiver can't be in the process of taking it since we guard the front dummy node with ACCESSED.
        let front_next = unsafe { front.next_node() };

        let front_next_guard = loop {
            // SAFETY: We may have shared access to front with a
            //         sender, who will never take.
            match unsafe { front_next.try_access() } {
                Ok(g) => break g,
                Err(_) => {}
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

        // Decrement the counter before releasing access.
        let mut len_guard = match self.len.0.lock() {
            Ok(g) => g,
            Err(p) => {
                error!("poison error: {:?}", p);
                p.into_inner()
            }
        };
        *len_guard -= 1;
        drop(len_guard);

        drop(front_next_guard);
        // front_next is no longer valid

        drop(dummy_front_guard);
        // dummy_front is no longer valid

        drop(front_guard);
        // we now have ownership of front

        // At this point, we detached the front node from the list and
        // released access to all resources.

        // SAFETY: We have exclusive access to front and it does not get dereferenced after the swap_take_drop call
        unsafe { front.swap_take_drop() }
    }

    pub fn push_back(&self, data: T) {
        let dummy_back = &self.dummy_back;
        let dummy_back_guard = loop {
            // SAFETY: Always safe to access a dummy node
            match unsafe { dummy_back.try_access() } {
                Ok(g) => break g,
                Err(_) => {}
            }
        };

        // SAFETY: we have access to dummy_back
        let back = unsafe { dummy_back.next_node() };
        // this could be the front_dummy, a node. irrelevant
        let back_guard = loop {
            // SAFETY: If this was a node being taken,
            // we wouldn't have access to dummy_back at this point. We do,
            // which means a pop is not in progress.
            match unsafe { back.try_access() } {
                Ok(g) => break g,
                Err(_) => {}
            }
        };

        let node = Node::new_leaked_node(data);

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

        // Before releasing access, update count and wake someone waiting.

        let mut len_guard = match self.len.0.lock() {
            Ok(g) => g,
            Err(p) => {
                error!("poison error: {:?}", p);
                p.into_inner()
            }
        };
        *len_guard += 1;
        drop(len_guard);
        self.len.1.notify_one();

        drop(back_guard);
        // back is no longer valid

        drop(dummy_back_guard);
        // dummy_back is no longer valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn empty_structure_valid() {
        let queue = ConcurrentBlockingList::<u8>::new();

        let dummy_front = &queue.dummy_front;
        assert_eq!(unsafe { dummy_front.identity() }, Identity::Front);

        let dummy_back = unsafe { dummy_front.next_node() };
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

        let front = unsafe { dummy_front.next_node() };
        assert_eq!(unsafe { front.identity() }, Identity::Node);
        assert_eq!(*unsafe { front.read_inner() }, v);

        let dummy_back = unsafe { front.next_node() };
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
}
