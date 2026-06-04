#[derive(Debug)]
pub enum SendError<T> {
    Closed(T),
}

#[derive(Debug)]
pub enum RecvError {
    Closed,
}

pub trait BlockingSend<T>
where
    Self: Clone,
{
    fn send(&self, data: T) -> Result<(), SendError<T>>;
}

pub trait BlockingReceive<T>
where
    Self: Clone,
{
    fn recv(&self) -> Result<T, RecvError>;
}

#[cfg(not(feature = "bench"))]
mod v1;

#[cfg(feature = "bench")]
pub mod v1;

#[cfg(feature = "v1")]
pub use v1::*;
