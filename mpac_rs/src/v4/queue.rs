use std::{
    alloc::Layout,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

// We need to know:
//   - # Receivers/Senders accessing queue
//   - If exclusive access is requested
//   - If any Receivers need to be woken

#[derive(Debug)]
pub struct AtomicQueue<T> {
    // Allocations only happen when a call stack
    // has exclusive control.
    buf: Buffer<T>,

    // cached
    len: AtomicUsize,
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

#[derive(Debug)]
enum Buffer<T> {
    Uninitialized,
    Array { ptr: NonNull<T>, cap: usize },
}

impl<T> AtomicQueue<T> {
    pub fn with_capacity(cap: usize) -> Self {
        let buf = match cap {
            0 => Buffer::Uninitialized,
            cap => {
                let ptr = Self::allocate(cap);
                Buffer::Array { ptr, cap }
            }
        };
        Self {
            buf,

            len: AtomicUsize::new(0),
        }
    }

    /// Creates an atomic queue.
    ///
    /// The use of `with_capacity` is preferred over `new`
    /// since reallocations are extremely expensive.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    pub fn len(&self) -> usize {
        self.len.load(Ordering::SeqCst)
    }

    pub fn start(&self) -> StartGuard<'_, T> {
        todo!()
    }

    pub fn end(&self) -> EndGuard<'_, T> {
        todo!()
    }

    pub fn exclusive(&self) -> ExclusiveGuard<'_, T> {
        todo!()
    }

    fn layout(n: usize) -> Layout {
        Layout::array::<T>(n).expect("could not allocate a buffer")
    }
    fn allocate(cap: usize) -> NonNull<T> {
        assert!(cap > 0, "cannot allocate 0 bytes");
        unsafe { NonNull::new_unchecked(std::alloc::alloc(Self::layout(cap)) as *mut T) }
    }
    /// Safety: This memory must be allocated and no longer accessed.
    unsafe fn deallocate(ptr: NonNull<T>, cap: usize) {
        unsafe {
            std::alloc::dealloc(ptr.as_ptr() as *mut u8, Self::layout(cap));
        }
    }
}

pub struct ExclusiveGuard<'a, T> {
    inner: &'a mut AtomicQueue<T>,
}

impl<T> ExclusiveGuard<'_, T> {
    pub fn reallocate(&self, cap: usize) {
        assert!(cap >= self.inner.len.load(Ordering::SeqCst));
        todo!()
    }

    pub fn release(&self) {
        todo!()
    }
}

impl<T> Drop for ExclusiveGuard<'_, T> {
    fn drop(&mut self) {
        self.release();
    }
}

pub struct StartGuard<'a, T> {
    inner: &'a AtomicQueue<T>,
}

impl<T> StartGuard<'_, T> {
    pub fn pop_wait(&self) -> T {
        todo!()
    }
    pub fn try_pop(&self) -> Option<T> {
        todo!()
    }

    pub fn release(&self) {
        todo!()
    }
}

impl<T> Drop for StartGuard<'_, T> {
    fn drop(&mut self) {
        self.release()
    }
}

pub struct EndGuard<'a, T> {
    inner: &'a AtomicQueue<T>,
}

impl<T> EndGuard<'_, T> {
    pub fn push_within_capacity(&self, data: T) {
        todo!()
    }
    pub fn release(&self) {
        todo!()
    }
}

impl<T> Drop for EndGuard<'_, T> {
    fn drop(&mut self) {
        self.release()
    }
}
