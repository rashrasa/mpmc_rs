use std::ptr::null;

use crate::v3::access_flag::{AccessFlag, Identity, ReleaseGuard, Status};

#[derive(Debug)]
pub struct Node<T> {
    flag: AccessFlag,
    /// This pointer can be dereferenced when the caller has verified that it's impossible
    /// for its corresponding Node to be TAKEN and has set the current flag to ACCESSED.
    next: *const Node<T>,
    inner: T,
}

impl<T: Send> Node<T> {
    pub fn new_node(data: T) -> Self {
        Node {
            inner: data,
            flag: AccessFlag::new(&Identity::Node),
            next: null(),
        }
    }

    pub fn new_front() -> Self {
        Node {
            inner: unsafe { std::mem::zeroed() },
            flag: AccessFlag::new(&Identity::Front),
            next: null(),
        }
    }

    pub fn new_back() -> Self {
        Node {
            inner: unsafe { std::mem::zeroed() },
            flag: AccessFlag::new(&Identity::Back),
            next: null(),
        }
    }

    /// SAFETY: Must already have access to self and next.
    pub unsafe fn set_next(&self, next: *const Node<T>) {
        unsafe {
            let node = (self as *const Node<T>).cast_mut();
            (*node).next = next;
        }
    }

    /// SAFETY: must never be read while in a TAKEN state
    pub unsafe fn next_node(&self) -> Option<&Node<T>> {
        let ptr = self.next;
        if ptr.is_null() {
            None
        } else {
            unsafe { Some(&*ptr) }
        }
    }

    /// SAFETY: must have exclusive access to node and
    /// all pointers to it have been dropped. This node must
    /// never be used again.
    pub unsafe fn swap_take_drop(&self) -> T {
        self.flag.try_take().expect("could not take front node");

        let mut front = unsafe { Box::from_raw((self as *const Node<T>).cast_mut()) };

        let mut v: T = unsafe { std::mem::zeroed() };

        std::mem::swap(&mut v, &mut front.inner);
        v
    }

    /// SAFETY: Must not be in the process of being TAKEN
    pub unsafe fn try_access(&self) -> Result<ReleaseGuard<'_>, Status> {
        self.flag.try_access()
    }

    /// SAFETY: Must not be in the process of being TAKEN
    pub unsafe fn try_declare_take(&self) -> Result<(), Status> {
        self.flag.try_declare_take()
    }

    /// SAFETY: Must not be in the process of being TAKEN
    pub unsafe fn identity(&self) -> Identity {
        self.flag.identity()
    }

    /// SAFETY: Must have exclusive access to this Node
    pub unsafe fn read_inner(&self) -> &T {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_structure_valid() {
        let node = Box::leak(Box::new(Node::new_node(5)));
        let front = Box::leak(Box::new(Node::new_front()));
        let back = Box::leak(Box::new(Node::new_back()));

        // SAFETY: We have exclusive access to all 3.
        unsafe {
            front.set_next(node);
            node.set_next(back);
            back.set_next(node);
        }

        assert_eq!(Identity::Front, front.flag.identity());
        assert_eq!(Identity::Back, back.flag.identity());
        assert_eq!(Identity::Node, node.flag.identity());

        // SAFETY: we own all 3
        unsafe {
            // Free up resources, unlikely to actually be needed since OS takes care of it
            let _ = Box::from_raw(front);
            let _ = Box::from_raw(back);
            let _ = Box::from_raw(node);
        }
    }
}
