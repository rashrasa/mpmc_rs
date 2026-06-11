#[derive(Debug)]
pub enum SendError<T> {
    Closed(T),
}

#[cfg(feature = "bench")]
#[derive(Debug)]
pub enum BSendError<T> {
    Closed((T, usize)),
}

#[derive(Debug)]
pub enum RecvError {
    Closed,
}

#[cfg(feature = "bench")]
#[derive(Debug)]
pub enum BRecvError {
    Closed(usize),
}

pub trait BlockingSend<T>
where
    Self: Clone + Send,
{
    #[cfg(feature = "bench")]
    fn b_send(&self, data: T) -> Result<usize, BSendError<T>>;

    fn send(&self, data: T) -> Result<(), SendError<T>>;
}

pub trait BlockingReceive<T>
where
    Self: Clone,
{
    #[cfg(feature = "bench")]
    fn b_recv(&self) -> Result<(T, usize), BRecvError>;

    fn recv(&self) -> Result<T, RecvError>;
}

#[cfg(feature = "bench")]
pub trait ChannelMaker {
    fn channel<T>(
        &self,
    ) -> (
        impl BlockingSend<T> + Send + 'static,
        impl BlockingReceive<T> + Send + 'static,
    )
    where
        Self: Sized,
        T: Send + 'static;
}

#[cfg(not(feature = "bench"))]
mod v1;

#[cfg(feature = "bench")]
pub mod v1;

#[cfg(feature = "v1")]
pub use v1::*;

#[cfg(not(feature = "bench"))]
mod v2;

#[cfg(feature = "bench")]
pub mod v2;

#[cfg(feature = "v2")]
pub use v2::*;
