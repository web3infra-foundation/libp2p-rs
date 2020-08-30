
use async_trait::async_trait;
use crate::transport::{Transport, TransportError, TransportListener};
use crate::Multiaddr;
use std::{fmt, io, pin::Pin};
use futures::{prelude::*, task::Context, task::Poll, StreamExt};

/// Implementation of `TransportListener` that supports async post-processing
///
/// After a connection has been accepted by the transport, it may need to go through
/// asynchronous post-processing (i.e. protocol upgrade negotiations). Such
/// post-processing should not block the `Listener` from producing the next
/// connection, hence further connection setup proceeds asynchronously.

pub struct AsyncListener<InnerListener>(InnerListener);

impl AsyncListener<InnerListener> {
    /// Wraps around a `TransportListener` to add async post-processing.
    pub fn new(inner: InnerListener) -> Self {
        AsyncListener { 0: inner }
    }
}

// impl Default for DummyTransport {
//     fn default() -> Self {
//         DummyTransport::new()
//     }
// }

#[async_trait]
impl<InnerListener: TransportListener> TransportListener for AsyncListener<InnerListener> {
    type Output = InnerListener::Output;
    type Error = InnerListener::Error;

    async fn accept(&self) -> Result<Self::Output, TransportError<Self::Error>> {

        let stream = stream::unfold(&self.0, |s| s.accept());

        stream.next().await

/*        let stream = futures::stream::unfold(r, move |mut r| async move {
            let mut len = [0; 4];
            if let Err(e) = r.read_exact(&mut len).await {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return None
                }
                return Some((Err(e), r))
            }
            let n = u32::from_be_bytes(len) as usize;
            if n > max_len {
                let msg = format!("data length {} exceeds allowed maximum {}", n, max_len);
                return Some((Err(io::Error::new(io::ErrorKind::PermissionDenied, msg)), r))
            }
            let mut v = vec![0; n];
            if let Err(e) = r.read_exact(&mut v).await {
                return Some((Err(e), r))
            }
            Some((Ok(v), r))
        });
*/


    }

    fn multi_addr(&self) -> Multiaddr {
        self.0.multi_addr()
    }
}