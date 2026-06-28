// similar to v4, except:
// - uses parking_lot concurrency primitives
// - uses a parking_lot::RwLock for the exclusivity mechanism instead of stateful reader/writer handles

use parking_lot::{Condvar, Mutex, RwLock};

use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
};

#[derive(Debug)]
pub struct AtomicQueueHandle<T> {
    inner: Arc<RwLock<AtomicQueue<T>>>,
    resizes: RefCell<usize>,
}

impl<T> Clone for AtomicQueueHandle<T> {
    fn clone(&self) -> Self {
        AtomicQueueHandle {
            inner: self.inner.clone(),
            resizes: self.resizes.clone(),
        }
    }
}

#[derive(Debug)]
pub struct AtomicQueue<T> {
    buf: Vec<Location<T>>,
    resizes: (Mutex<usize>, Condvar, AtomicBool),

    start: AtomicIsize,
    end: AtomicIsize,
    len: AtomicIsize,
}

impl<T> AtomicQueue<T> {
    pub fn with_capacity(cap: usize) -> AtomicQueueHandle<T> {
        let buf = vec![
            Location {
                inner: Mutex::new(None),
                waker: Condvar::new()
            };
            cap
        ];
        let resizes = (Mutex::new(0), Condvar::new(), AtomicBool::new(false));
        let inner = AtomicQueue {
            buf,
            resizes,
            start: AtomicIsize::new(0),
            end: AtomicIsize::new(0),
            len: AtomicIsize::new(0),
        };

        let inner = Arc::new(parking_lot::RwLock::new(inner));

        AtomicQueueHandle {
            inner,
            resizes: RefCell::new(0),
        }
    }
}

impl<T> AtomicQueueHandle<T> {
    fn reallocate(vec: &mut Vec<Location<T>>, start: usize, end: usize, cap: usize) {
        let old_len = vec.len();
        assert!(cap >= old_len, "attempted to allocate less capacity");

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

            for (e, v) in vec[0..start_len].iter_mut().zip(start_e) {
                *e = v;
            }

            let end_e = old.drain(0..=end);

            for (e, v) in vec[start_len..=start_len + end].iter_mut().zip(end_e) {
                *e = v;
            }
        } else {
            let elements = old.drain(start..=end);

            for (e, v) in vec.iter_mut().zip(elements) {
                *e = v;
            }
        }
    }

    pub fn pop_front_wait_with_check<F>(&self, check: F) -> Result<T, ReaderError>
    where
        F: FnMut() -> bool,
    {
        let inner = self.inner.read();
        let index = inner.start.fetch_add(1, Ordering::SeqCst) as usize % inner.buf.len();

        let location = inner.buf.get(index).unwrap();

        if let Ok(val) = location.take_wait_with_check(check) {
            inner.len.fetch_sub(1, Ordering::SeqCst);

            Ok(val)
        } else {
            Err(ReaderError::CheckFailed)
        }
    }

    pub fn push(&self, data: T) {
        loop {
            let inner = self.inner.upgradable_read();
            let len = inner.buf.len();
            let end = inner.end.fetch_add(1, Ordering::SeqCst) as usize % len;
            let mut guard = inner.buf[end].inner.lock();

            if guard.is_some() {
                drop(guard);
                drop(inner);
                // Queue detected full at this instant.
                // Wait for resize and retry.
                self.request_resize_block();
                continue;
            }

            *guard = Some(data);
            drop(guard);

            inner.buf[end].waker.notify_one();

            inner.len.fetch_add(1, Ordering::SeqCst);

            return;
        }
    }

    fn request_resize_block(&self) {
        let mut reader = self.inner.upgradable_read();

        let mut known_resizes = self.resizes.borrow_mut();
        match reader
            .resizes
            .2
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => {
                let resizes = *reader.resizes.0.lock();
                if resizes != *known_resizes {
                    // It appears a resize has occurred already.
                    *known_resizes = resizes;
                    return;
                }

                // we must perform the resize
                reader.with_upgraded(|inner| {
                    let len = inner.len.load(Ordering::SeqCst);
                    let start = inner.start.load(Ordering::SeqCst).rem_euclid(len) as usize;
                    let end = inner.end.load(Ordering::SeqCst).rem_euclid(len) as usize;
                    let old_cap = inner.buf.len();
                    AtomicQueueHandle::reallocate(&mut inner.buf, start, end, old_cap * 2);
                });

                *reader.resizes.0.lock() += 1;
                reader.resizes.1.notify_all();
            }
            Err(_) => loop {
                // Someone else succeeded, we must wait for the resize to update.
                let mut guard = reader.resizes.0.lock();
                while *guard == *known_resizes {
                    reader.resizes.1.wait(&mut guard);
                }
                let val = *guard;
                drop(guard);

                *known_resizes = val;
            },
        }
    }

    pub fn len(&self) -> usize {
        self.inner.read().len.load(Ordering::SeqCst).max(0) as usize
    }
}

#[derive(Debug)]
struct Location<T> {
    inner: Mutex<Option<T>>,
    waker: Condvar,
}

impl<T> Location<T> {
    fn take_wait_with_check<F>(&self, mut check: F) -> Result<T, ()>
    where
        F: FnMut() -> bool,
    {
        let mut inner = self.inner.lock();

        while inner.is_none() {
            if !check() {
                return Err(());
            }
            self.waker.wait(&mut inner);
        }

        let v = inner
            .take()
            .expect("unexpected None value after waiting for Some");
        drop(inner);
        self.waker.notify_one();

        Ok(v)
    }
    #[allow(unused)]
    fn try_take(&self) -> Option<T> {
        let mut inner = self.inner.lock();
        let v = inner.take();
        drop(inner);
        self.waker.notify_one();

        v
    }
}

impl<T> Clone for Location<T> {
    fn clone(&self) -> Self {
        Location {
            inner: Mutex::new(None),
            waker: Condvar::new(),
        }
    }
}

#[derive(Debug)]
pub enum ReaderError {
    CheckFailed,
}
