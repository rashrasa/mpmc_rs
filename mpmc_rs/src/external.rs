#[cfg(feature = "bench")]
pub struct CrossbeamMaker;
#[cfg(feature = "bench")]
impl crate::BChannelMaker for CrossbeamMaker {
    fn channel<T>(
        &self,
    ) -> (
        impl crate::BBlockingSend<T> + Send + Clone + 'static,
        impl crate::BBlockingReceive<T> + Send + Clone + 'static,
    )
    where
        T: Send + 'static,
    {
        crossbeam::channel::unbounded()
    }
}

impl<T: Send> crate::BlockingSend<T> for crossbeam::channel::Sender<T> {
    fn send(&self, data: T) -> Result<(), crate::SendError<T>> {
        crossbeam::channel::Sender::send(self, data).map_err(|e| crate::SendError::Closed(e.0))
    }
}

impl<T: Send> crate::BBlockingSend<T> for crossbeam::channel::Sender<T> {
    fn b_send(&self, data: T) -> Result<usize, crate::BSendError<T>> {
        let len = self.len();
        match crossbeam::channel::Sender::send(self, data) {
            Ok(_) => Ok(len),
            Err(e) => Err(crate::BSendError::Closed((e.0, len))),
        }
    }
}

impl<T: Send> crate::BlockingReceive<T> for crossbeam::channel::Receiver<T> {
    fn recv(&self) -> Result<T, crate::RecvError> {
        crossbeam::channel::Receiver::recv(self).map_err(|_| crate::RecvError::Closed)
    }
}

impl<T: Send> crate::BBlockingReceive<T> for crossbeam::channel::Receiver<T> {
    fn b_recv(&self) -> Result<(T, usize), crate::BRecvError> {
        let len = self.len();

        match crossbeam::channel::Receiver::recv(self) {
            Ok(v) => Ok((v, len)),
            Err(_) => Err(crate::BRecvError::Closed(len)),
        }
    }
}
