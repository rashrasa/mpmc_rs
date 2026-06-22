use std::{
    ptr::NonNull,
    sync::{Condvar, Mutex},
};

use log::error;

use crate::{
    LogAndLock,
    v3::{
        access_flag::{Identity, Status},
        node::Node,
    },
};

unsafe impl<T: Send> Send for ConcurrentBlockingList<T> {}
unsafe impl<T: Send> Sync for ConcurrentBlockingList<T> {}

// INVARIANT: front and back are never TAKEN
#[derive(Debug)]
pub struct ConcurrentBlockingList<T> {
    dummy_front: NonNull<Node<T>>,
    dummy_back: NonNull<Node<T>>,

    len: (Mutex<usize>, Condvar),
}

impl<T: Send> ConcurrentBlockingList<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dummy_front(&self) -> &Node<T> {
        // Safety: This is always safe as all necessary
        // modifications for DUMMY nodes are done
        // safely with shared references through atomic flag access checks.
        unsafe { self.dummy_front.as_ref() }
    }

    pub fn dummy_back(&self) -> &Node<T> {
        // Safety: Same as dummy_front
        unsafe { self.dummy_back.as_ref() }
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
        let dummy_front = self.dummy_front();
        let dummy_front_guard = loop {
            // SAFETY: Always safe to try_access dummy nodes
            if let Ok(g) = unsafe { dummy_front.try_access() } {
                break g;
            }
        };

        // SAFETY: We have access to dummy front. No other receiver will get through.
        // A sender may be in the middle of updating its next pointer
        // but senders never take.
        let front = unsafe { dummy_front.next_node() }.unwrap();

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
            let dummy_back = self.dummy_back();
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

    pub fn len(&self) -> usize {
        *self.len.0.lock().log_and_lock()
    }
}

impl<T: Send> Default for ConcurrentBlockingList<T> {
    fn default() -> Self {
        // This is safe by construction. Node is allocated on the heap and will be valid until freed manually
        let mut dummy_front =
            unsafe { NonNull::new_unchecked(Box::leak(Box::new(Node::new_front()))) };
        let mut dummy_back =
            unsafe { NonNull::new_unchecked(Box::leak(Box::new(Node::new_back()))) };

        // SAFETY: We have exclusive access to both
        unsafe {
            dummy_back.as_mut().set_next(dummy_front.as_ref());
            dummy_front.as_mut().set_next(dummy_back.as_ref());
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

        let dummy_front = queue.dummy_front();
        assert_eq!(unsafe { dummy_front.identity() }, Identity::Front);

        let dummy_back = unsafe { dummy_front.next_node() }.unwrap();
        assert_eq!(unsafe { dummy_back.identity() }, Identity::Back);

        let dummy_back_list = queue.dummy_back();
        assert_eq!(unsafe { dummy_back_list.identity() }, Identity::Back);
    }

    #[test]
    fn push_back_structure_valid() {
        let queue = ConcurrentBlockingList::<u8>::new();

        let v = 5;
        queue.push_back(v);

        let dummy_front = queue.dummy_front();
        assert_eq!(unsafe { dummy_front.identity() }, Identity::Front);

        let front = unsafe { dummy_front.next_node() }.unwrap();
        assert_eq!(unsafe { front.identity() }, Identity::Node);
        assert_eq!(*unsafe { front.read_inner() }, v);

        let dummy_back = unsafe { front.next_node() }.unwrap();
        assert_eq!(unsafe { dummy_back.identity() }, Identity::Back);

        let dummy_back_list = queue.dummy_back();
        assert_eq!(unsafe { dummy_back_list.identity() }, Identity::Back);
    }
}
