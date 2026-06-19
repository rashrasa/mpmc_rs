use std::{
    cell::UnsafeCell,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicIsize, AtomicUsize, Ordering},
    },
};

use log::warn;

use crate::{
    LogAndLock,
    v4::{
        action::{Action, AtomicAction},
        status::{AtomicStatus, Status},
    },
};

// We need to know:
//   - # Receivers/Senders accessing queue
//   - If exclusive access is requested
//   - If any Receivers need to be woken

#[derive(Debug)]
/// Queue where push and pop operations are atomic.
///
/// While it is named AtomicQueue, it uses mutexes for
/// state management.
///
/// This queue keeps track of readers and writers, a state
/// flag for each memory location, and start/end indexes.
///
/// The point of this queue is to sacrifice memory efficiency
/// and tail latency for amortized throughput.
pub struct AtomicQueue<T> {
    // Allocations only happen when a call stack
    // has exclusive control.
    buf: UnsafeCell<Vec<Location<T>>>,

    // more of a "suggestion" rather than an explicit
    // start / end. When an index is returned, the
    // memory location needs to be checked for the
    // existence of a value.
    start: AtomicIsize,
    end: AtomicIsize,

    len: AtomicUsize,

    access: AccessTracker,
}

// Safety: AtomicQueue owns a unique pointer to a
// heap-allocated array of T's which is always valid.
//
// T needs to be Send since ownership of all contained T
// values will also be transferred.
unsafe impl<T: Send> Send for AtomicQueue<T> {}

// Safety: All operations are managed with access control
// mechanisms that use atomics. Operations that need exclusive access,
// like re-allocations, have to wait until all shared accesses are dropped.
//
// T needs to be Send since queue operations involving transferring ownership of
// T values are callable from a shared borrow.
//
// T does not need to be Sync since no borrows are made inside the queue, only ownership
// transfers.
unsafe impl<T: Send> Sync for AtomicQueue<T> {}

impl<T> AtomicQueue<T> {
    pub fn with_capacity(cap: usize) -> Self {
        let buf = vec![
            Location {
                inner: Mutex::new(None),
                waker: Condvar::new()
            };
            cap
        ];
        let status = match cap {
            0 => Status::Uninitialized,
            _ => Status::Active,
        };
        Self {
            buf: UnsafeCell::new(buf),
            start: AtomicIsize::new(0),
            end: AtomicIsize::new(0),

            len: AtomicUsize::new(0),

            access: AccessTracker {
                status: Arc::new(AtomicStatus::new(status)),
                readers: Mutex::new(Vec::with_capacity(5)),
                writers: Mutex::new(Vec::with_capacity(5)),
            },
        }
    }

    /// Creates an atomic queue.
    ///
    /// The use of `with_capacity` is preferred over `new`
    /// since reallocations are extremely expensive.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    pub fn reader(queue: Arc<AtomicQueue<T>>) -> ReaderAccessHandle<T> {
        let desc = Arc::new(AccessDescriptor {
            action: AtomicAction::new(Action::Idle),
        });

        queue
            .access
            .readers
            .lock()
            .log_and_lock()
            .push(Arc::clone(&desc));

        ReaderAccessHandle { queue, desc }
    }

    pub fn writer(queue: Arc<AtomicQueue<T>>) -> WriterAccessHandle<T> {
        let desc = Arc::new(AccessDescriptor {
            action: AtomicAction::new(Action::Idle),
        });

        queue
            .access
            .writers
            .lock()
            .log_and_lock()
            .push(Arc::clone(&desc));

        WriterAccessHandle { queue, desc }
    }

    fn reallocate(vec: &mut Vec<Location<T>>, start: usize, end: usize, cap: usize) {
        let old_len = vec.len();
        assert!(cap >= old_len, "attempted to allocate less capacity");

        // At this point, we should be the only one
        // accessing the vec.

        // All Valid items should be in one contiguous range.

        let mut old = vec![
            Location {
                inner: Mutex::new(None),
                waker: Condvar::new()
            };
            cap
        ];

        std::mem::swap(vec, &mut old);

        if start > end {
            let start_len = old_len - start;
            let start_e = old.drain(start..old_len);

            for (e, v) in (&mut vec[0..start_len]).iter_mut().zip(start_e) {
                *e = v;
            }

            let end_e = old.drain(0..=end);

            for (e, v) in (&mut vec[start_len..=end]).iter_mut().zip(end_e) {
                *e = v;
            }
        } else {
            let elements = old.drain(start..=end);

            for (e, v) in vec.iter_mut().zip(elements) {
                *e = v;
            }
        }
    }
}

#[derive(Debug)]
struct Location<T> {
    inner: Mutex<Option<T>>,
    waker: Condvar,
}

impl<T> Location<T> {
    fn take(&self) -> T {
        let mut inner = self.inner.lock().log_and_lock();

        while inner.is_none() {
            inner = self.waker.wait(inner).log_and_lock();
        }

        let v = inner
            .take()
            .expect("unexpected None value after waiting for Some");
        drop(inner);
        self.waker.notify_one();

        v
    }
}

// This implementation is only done for use in vec![]
impl<T> Clone for Location<T> {
    fn clone(&self) -> Self {
        Location {
            inner: Mutex::new(None),
            waker: Condvar::new(),
        }
    }
}

// Stateful access
#[derive(Debug)]
struct AccessTracker {
    // used to avoid races when
    // declaring a resize being needed
    status: Arc<AtomicStatus>,

    readers: Mutex<Vec<Arc<AccessDescriptor>>>,
    writers: Mutex<Vec<Arc<AccessDescriptor>>>,
}

// For the Queue

#[derive(Debug)]
struct AccessDescriptor {
    action: AtomicAction,
}

// For the 'User'

#[derive(Debug)]
pub struct ReaderAccessHandle<T> {
    queue: Arc<AtomicQueue<T>>,
    desc: Arc<AccessDescriptor>,
}

impl<T> ReaderAccessHandle<T> {
    pub fn pop_front_wait(&self) -> T {
        loop {
            // attempt to update own action
            if let Err(e) = self.desc.action.compare_exchange_weak(
                Action::Idle,
                Action::Reading,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                match e {
                    Action::Idle => continue, // TODO: check if spurious failure makes sense here
                    Action::ExternallyBlocked => continue,
                    a => unreachable!("unexpected action flag for reader: {:?}", a),
                }
            } else {
                break;
            }
        }

        let queue = &self.queue;

        // Safety: Here, we are only allowed a shared reference into Vec since there will be
        // other readers / writers active.
        let buf = self.queue.buf.get() as *const Vec<Location<T>>;
        let buf = unsafe { &*buf };

        let index = queue.start.fetch_add(1, Ordering::SeqCst) as usize % buf.len();

        let val = buf.get(index).unwrap().take();

        if let Err(e) = self.desc.action.compare_exchange(
            Action::Reading,
            Action::Idle,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            unreachable!(
                "action flag for reader flipped while in critical section {:?}",
                e
            )
        }
        self.queue.len.fetch_sub(1, Ordering::SeqCst);
        val
    }

    pub fn len(&self) -> usize {
        self.queue.len.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub struct WriterAccessHandle<T> {
    queue: Arc<AtomicQueue<T>>,
    desc: Arc<AccessDescriptor>,
}

impl<T> WriterAccessHandle<T> {
    fn exclusive(&self, original_status: Status) -> ExclusiveGuard<'_, T> {
        // prevent creating / dropping readers and writers
        let readers = self.queue.access.readers.lock().log_and_lock();
        let writers = self.queue.access.readers.lock().log_and_lock();

        let mut waiting = 0;

        for reader in readers.iter() {
            loop {
                match reader.action.compare_exchange_weak(
                    Action::Idle,
                    Action::ExternallyBlocked,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(e) => match e {
                        Action::ResizeRequested => {
                            warn!("a reader requested a resize");
                            continue;
                        }
                        Action::Idle | Action::Reading => {
                            continue;
                        }
                        Action::ExternallyBlocked => {
                            unreachable!(
                                "someone else has blocked this reader. potential deadlock"
                            );
                        }
                        Action::Writing => {
                            unreachable!("reader was found writing");
                        }
                    },
                }
            }
        }

        for writer in writers.iter() {
            loop {
                match writer.action.compare_exchange_weak(
                    Action::Idle,
                    Action::ExternallyBlocked,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(e) => match e {
                        Action::ResizeRequested => {
                            // here, we have detected ourselves
                            waiting += 1;
                        }
                        Action::Idle | Action::Writing => {
                            continue;
                        }
                        Action::ExternallyBlocked => {
                            unreachable!(
                                "someone else has blocked this writer. potential deadlock"
                            );
                        }
                        Action::Reading => {
                            unreachable!("writer was found reading");
                        }
                    },
                }
            }
        }

        if waiting != 1 {
            warn!("resize requester count {} is not equal to 1", waiting);
        }

        // At this point, all readers and writers have acknowledged
        // the request to get exclusive access and are waiting for the
        // queue to be released.

        let queue = unsafe { &mut *self.queue.buf.get() };
        ExclusiveGuard {
            inner: &self.queue,
            buf: queue,
            original_status,
        }
    }

    pub fn request_resize_block(&self) {
        let mut status = self.queue.access.status.load(Ordering::SeqCst);
        loop {
            match self.queue.access.status.compare_exchange_weak(
                status,
                Status::WaitingToResize,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    // we are the ones to update the value, we must perform the resize.
                    let mut x = self.exclusive(status);
                    let new_cap = x.buf.len() * 2;
                    x.reallocate(new_cap);
                    break;
                }
                Err(s) => match s {
                    Status::Active => {
                        status = Status::Active;
                    }
                    Status::Uninitialized => {
                        // We could only get here if we initially loaded
                        // Active, then it updated to Uninitialized.

                        unreachable!("queue was de-initialized unexpectedly");
                    }
                    Status::WaitingToResize => {
                        // we are not the ones to update the value, we should wait for the resize to complete.
                        loop {
                            let new_status = self.queue.access.status.load(Ordering::SeqCst);

                            // status was restored
                            if status.code() == new_status.code() {
                                break;
                            }

                            // TODO: Implement waking mechanism
                            std::hint::spin_loop();
                        }
                    }
                },
            }
        }
    }

    pub fn push(&self, data: T) {
        loop {
            // attempt to update own action
            if let Err(e) = self.desc.action.compare_exchange_weak(
                Action::Idle,
                Action::Writing,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                match e {
                    Action::Idle => continue, // TODO: check if spurious failure makes sense here
                    Action::ExternallyBlocked => continue,
                    a => unreachable!("unexpected action flag for reader: {:?}", a),
                }
            } else {
                break;
            }
        }

        loop {
            let queue = &self.queue;

            // Safety: Here, we are only allowed a shared reference into Vec since there will be
            // other readers / writers active.
            let buf = self.queue.buf.get() as *const Vec<Location<T>>;
            let buf = unsafe { &*buf };
            let len = buf.len();

            let end = queue.end.fetch_add(1, Ordering::SeqCst) as usize % len;

            // Queue detected full at this instant
            let location = &buf[end];
            let mut guard = location.inner.lock().log_and_lock();
            if guard.is_some() {
                drop(guard);
                if let Err(e) = self.desc.action.compare_exchange_weak(
                    Action::Writing,
                    Action::ResizeRequested,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    unreachable!("writer action flag was updated while Writing to {:?}", e);
                }
                self.request_resize_block();
                continue;
            } else {
                *guard = Some(data);
                drop(guard);
                location.waker.notify_one();
                self.desc.action.store(Action::Idle, Ordering::SeqCst);
                self.queue.len.fetch_add(1, Ordering::SeqCst);
                return;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len.load(Ordering::SeqCst)
    }
}

pub struct ExclusiveGuard<'a, T> {
    inner: &'a AtomicQueue<T>,
    buf: &'a mut Vec<Location<T>>,
    original_status: Status,
}

impl<T> ExclusiveGuard<'_, T> {
    pub fn reallocate(&mut self, cap: usize) {
        let old_len = self.buf.len();
        let start = self.inner.start.load(Ordering::SeqCst) as usize % old_len;
        let end = self.inner.end.load(Ordering::SeqCst) as usize % old_len;
        AtomicQueue::reallocate(self.buf, start, end, cap);
    }

    pub fn release(&mut self) {
        let status = self.inner.access.status.load(Ordering::SeqCst);
        match status {
            Status::Uninitialized | Status::Active => {
                unreachable!(
                    "unexpected status {:?} while attempting to release exclusive guard",
                    status
                );
            }
            Status::WaitingToResize => {
                self.inner
                    .access
                    .status
                    .store(self.original_status, Ordering::SeqCst);
            }
        }
    }
}

impl<T> Drop for ExclusiveGuard<'_, T> {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn single_item() {
        let item = 82;

        let queue: Arc<AtomicQueue<i32>> = Arc::new(AtomicQueue::with_capacity(23));

        let writer = AtomicQueue::writer(Arc::clone(&queue));
        let reader = AtomicQueue::reader(Arc::clone(&queue));
        std::thread::spawn(move || writer.push(item));

        let v = reader.pop_front_wait();

        assert_eq!(item, v);
    }
}
