use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug)]
pub struct AtomicAction {
    inner: AtomicU8,
}

impl AtomicAction {
    pub fn new(action: Action) -> Self {
        let code = action.code();
        Self {
            inner: AtomicU8::new(code),
        }
    }

    pub fn load(&self, ordering: Ordering) -> Action {
        Action::from_code(self.inner.load(ordering))
    }

    pub fn store(&self, action: Action, ordering: Ordering) {
        self.inner.store(action.code(), ordering)
    }

    pub fn compare_exchange(
        &self,
        current: Action,
        new: Action,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Action, Action> {
        match self
            .inner
            .compare_exchange(current.code(), new.code(), success, failure)
        {
            Ok(code) => Ok(Action::from_code(code)),
            Err(code) => Err(Action::from_code(code)),
        }
    }

    pub fn compare_exchange_weak(
        &self,
        current: Action,
        new: Action,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Action, Action> {
        match self
            .inner
            .compare_exchange_weak(current.code(), new.code(), success, failure)
        {
            Ok(code) => Ok(Action::from_code(code)),
            Err(code) => Err(Action::from_code(code)),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Action {
    Idle,
    Reading,
    Writing,
    ResizeRequested,
    ExternallyBlocked,
}

impl Action {
    pub fn code(&self) -> u8 {
        match self {
            Action::Idle => 0,
            Action::Reading => 1,
            Action::Writing => 2,
            Action::ResizeRequested => 3,
            Action::ExternallyBlocked => 4,
        }
    }

    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Action::Idle,
            1 => Action::Reading,
            2 => Action::Writing,
            3 => Action::ResizeRequested,
            4 => Action::ExternallyBlocked,
            _ => unimplemented!(),
        }
    }
}
