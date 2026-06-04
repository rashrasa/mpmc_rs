#[derive(Debug)]
pub enum SendError<T> {
    Closed(T),
}

#[derive(Debug)]
pub enum RecvError {
    Closed,
}

pub trait ChannelSend<T> {
    fn send(&self, data: T) -> Result<(), SendError<T>>;
}

pub trait ChannelReceive<T> {
    fn recv(&self) -> Result<T, RecvError>;
}

#[cfg(not(feature = "bench"))]
mod v1;

#[cfg(feature = "bench")]
pub mod v1;

#[cfg(feature = "v1")]
pub use v1::*;
