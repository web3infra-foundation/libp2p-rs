use libp2p_traits::{ReadEx, WriteEx};
use std::{fmt, io};

/// `Stream` & `Sink` that reads and writes a length prefix in front of the actual data.
pub struct LengthPrefixSocket<T> {
    inner: T,
    max_frame_len: usize,
}

impl<T> fmt::Debug for LengthPrefixSocket<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("LengthPrefixSocket")
    }
}

impl<T> LengthPrefixSocket<T>
where
    T: ReadEx + WriteEx + Send + 'static,
{
    /// create a new LengthPrefixSocket
    pub fn new(socket: T, max_len: usize) -> Self {
        Self {
            inner: socket,
            max_frame_len: max_len,
        }
    }

    /// convenient method for reading a whole frame
    #[allow(clippy::let_unit_value)]
    pub async fn recv_frame(&mut self) -> io::Result<Vec<u8>> {
        let mut len = [0; 4];
        let _ = self.inner.read_exact2(&mut len).await?;

        let n = u32::from_be_bytes(len) as usize;
        if n > self.max_frame_len {
            let msg = format!("data length {} exceeds allowed maximum {}", n, self.max_frame_len);
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, msg));
        }

        let mut frame = vec![0; n];
        self.inner.read_exact2(&mut frame).await?;

        Ok(frame)
    }

    /// sending a length delimited frame
    pub async fn send_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        // write flush can reduce the probability of secuio read failed Because split bug
        // especially for test case of secuio + muxer
        use bytes::{BufMut, BytesMut};
        let mut buf = BytesMut::with_capacity(frame.len() + 4);
        buf.put_u32(frame.len() as u32);
        buf.put(frame);
        self.inner.write_all2(&buf).await

        // self.inner
        //     .write_all2(&(frame.len() as u32).to_be_bytes())
        //     .await?;
        // self.inner.write_all2(&frame).await
    }

    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush2().await
    }
    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.inner.close2().await
    }
}
