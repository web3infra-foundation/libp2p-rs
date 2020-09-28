// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! Transports with timeouts on the connection setup.
//!
//! The connection setup includes all protocol upgrades applied on the
//! underlying `Transport`.
// TODO: add example

use crate::transport::TransportListener;
use crate::{transport::TransportError, Multiaddr, Transport};
use async_trait::async_trait;
use futures::future::{select, Either};
use futures_timer::Delay;
use log::trace;
use std::time::Duration;

/// A `TransportTimeout` is a `Transport` that wraps another `Transport` and adds
/// timeouts to all inbound and outbound connection attempts.
///
/// **Note**: `listen_on` is never subject to a timeout, only the setup of each
/// individual accepted connection.
#[derive(Debug, Copy, Clone)]
pub struct TransportTimeout<InnerTrans> {
    inner: InnerTrans,
    outgoing_timeout: Duration,
    incoming_timeout: Duration,
}

impl<InnerTrans> TransportTimeout<InnerTrans> {
    /// Wraps around a `Transport` to add timeouts to all the sockets created by it.
    pub fn new(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: timeout,
            incoming_timeout: timeout,
        }
    }

    /// Wraps around a `Transport` to add timeouts to the outgoing connections.
    pub fn with_outgoing_timeout(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: timeout,
            incoming_timeout: Duration::from_secs(100 * 365 * 24 * 3600), // 100 years
        }
    }

    /// Wraps around a `Transport` to add timeouts to the ingoing connections.
    pub fn with_ingoing_timeout(trans: InnerTrans, timeout: Duration) -> Self {
        TransportTimeout {
            inner: trans,
            outgoing_timeout: Duration::from_secs(100 * 365 * 24 * 3600), // 100 years
            incoming_timeout: timeout,
        }
    }
}

#[async_trait]
impl<InnerTrans: Transport> Transport for TransportTimeout<InnerTrans> {
    type Output = InnerTrans::Output;
    type Listener = TimeoutListener<InnerTrans::Listener>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError> {
        let listener = self.inner.listen_on(addr)?;

        let listener = TimeoutListener {
            inner: listener,
            timeout: self.incoming_timeout,
        };

        Ok(listener)
    }

    async fn dial(self, addr: Multiaddr) -> Result<Self::Output, TransportError> {
        let output = select(self.inner.dial(addr), Delay::new(self.outgoing_timeout)).await;
        match output {
            Either::Left((stream, _)) => {
                trace!("dialing connected first");
                Ok(stream?)
            }
            Either::Right(_) => {
                trace!("dialing timeout first");
                Err(TransportError::Timeout)
            }
        }
        // let mut dial = self.inner.dial(addr).fuse();
        // let mut timeout = Delay::new(self.outgoing_timeout).fuse();
        //
        // select! {
        //     stream = dial => {
        //         trace!("connected first");
        //         let stream = stream?;
        //         Ok(stream)
        //     },
        //     _ = timeout => {
        //         trace!("dial timeout first");
        //         Err(TransportError::Timeout)
        //     }
        // }
    }
}

pub struct TimeoutListener<InnerListener> {
    inner: InnerListener,
    timeout: Duration,
}

#[async_trait]
impl<InnerListener: TransportListener> TransportListener for TimeoutListener<InnerListener> {
    type Output = InnerListener::Output;

    async fn accept(&mut self) -> Result<Self::Output, TransportError> {
        let output = select(self.inner.accept(), Delay::new(self.timeout)).await;
        match output {
            Either::Left((stream, _)) => {
                trace!("accepted first");
                Ok(stream?)
            }
            Either::Right(_) => {
                trace!("accept timeout first");
                Err(TransportError::Timeout)
            }
        }
        // let mut accept = self.inner.accept().fuse();
        // let mut timeout = Delay::new(self.timeout).fuse();
        //
        // select! {
        //     stream = accept => {
        //         trace!("accept first");
        //         let stream = stream?;
        //         Ok(stream)
        //     },
        //     _ = timeout => {
        //         trace!("dial timeout first");
        //         Err(TransportError::Timeout)
        //     }
        // }
    }

    fn multi_addr(&self) -> Multiaddr {
        self.inner.multi_addr()
    }
}

#[cfg(test)]
mod tests {
    use crate::transport::memory::MemoryTransport;
    use crate::transport::TransportListener;
    use crate::{Multiaddr, Transport};
    use std::time::Duration;

    #[test]
    fn dialer_and_listener_timeout() {
        fn test1(addr: Multiaddr) {
            futures::executor::block_on(async move {
                let mut timeout_listener = MemoryTransport::default().timeout(Duration::from_secs(1)).listen_on(addr).unwrap();
                assert!(timeout_listener.accept().await.is_err());
            });
        }

        fn test2(addr: Multiaddr) {
            futures::executor::block_on(async move {
                let tcp = MemoryTransport::default().timeout(Duration::from_secs(1));
                assert!(tcp.dial(addr.clone()).await.is_err());
            });
        }

        test1("/memory/1111".parse().unwrap());
        test2("/memory/1111".parse().unwrap());
    }
}
