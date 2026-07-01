use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug)]
pub struct AtomicStatus {
    inner: AtomicU8,
}

impl AtomicStatus {
    pub fn new(status: Status) -> Self {
        let code = status.code();
        Self {
            inner: AtomicU8::new(code),
        }
    }

    pub fn load(&self, ordering: Ordering) -> Status {
        Status::from_code(self.inner.load(ordering))
    }

    pub fn store(&self, status: Status, ordering: Ordering) {
        self.inner.store(status.code(), ordering)
    }

    pub fn compare_exchange(
        &self,
        current: Status,
        new: Status,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Status, Status> {
        match self
            .inner
            .compare_exchange(current.code(), new.code(), success, failure)
        {
            Ok(code) => Ok(Status::from_code(code)),
            Err(code) => Err(Status::from_code(code)),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Status {
    Uninitialized,
    Active,
    // When a reader or writer acknowledges this status,
    // they must never be in the queue.
    //
    // Whoever sets this status has to be the one
    // to perform the update.
    WaitingToResize,
}

impl Status {
    pub const fn code(&self) -> u8 {
        match self {
            Status::Uninitialized => 0,
            Status::Active => 1,
            Status::WaitingToResize => 2,
        }
    }

    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Status::Uninitialized,
            1 => Status::Active,
            2 => Status::WaitingToResize,
            _ => unimplemented!(),
        }
    }
}
