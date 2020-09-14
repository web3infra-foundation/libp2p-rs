pub mod split;

use crate::{Read2, Write2};

// use futures::{AsyncRead, AsyncReadExt, AsyncWrite, io::{ReadHalf, WriteHalf}};
pub use split::{ReadHalf, WriteHalf};

pub trait ReadExt2: Read2 {
    /// Helper method for splitting this read/write object into two halves.
    ///
    /// The two halves returned implement the `AsyncRead` and `AsyncWrite`
    /// traits, respectively.
    ///
    /// # Examples
    ///
    /// ```
    /// # async_std::task::block_on(async {
    /// use futures::io::{self, AsyncReadExt, Cursor};
    ///
    /// // Note that for `Cursor` the read and write halves share a single
    /// // seek position. This may or may not be true for other types that
    /// // implement both `AsyncRead` and `AsyncWrite`.
    ///
    /// let reader = Cursor::new([1, 2, 3, 4]);
    /// let mut buffer = Cursor::new(vec![0, 0, 0, 0, 5, 6, 7, 8]);
    /// let mut writer = Cursor::new(vec![0u8; 5]);
    ///
    /// {
    ///     let (buffer_reader, mut buffer_writer) = (&mut buffer).split();
    ///     io::copy(reader, &mut buffer_writer).await?;
    ///     io::copy(buffer_reader, &mut writer).await?;
    /// }
    ///
    /// assert_eq!(buffer.into_inner(), [1, 2, 3, 4, 5, 6, 7, 8]);
    /// assert_eq!(writer.into_inner(), [5, 6, 7, 8, 0]);
    /// # Ok::<(), Box<dyn std::error::Error>>(()) }).unwrap();
    /// ```
    fn split2(self) -> (ReadHalf<Self>, WriteHalf<Self>)
    where
        Self: Sized + Write2 + Send + Unpin,
    {
        split::split(self)
    }
}

impl<R: Read2 + ?Sized> ReadExt2 for R {}

#[cfg(test)]
mod tests {

    use async_std::net::{TcpListener, TcpStream};
    use async_std::task;
    use super::{Read2, ReadExt2, Write2};

    #[test]
    fn test_split() {
        task::block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("local_addr");

            let _server = task::spawn(async move {
                let (s, _addr) = listener.accept().await.expect("accept");
                let (mut reader, mut writer) = s.split2();

                task::spawn(async move {
                    let mut buf = vec![0; 512];
                    loop {
                        let n = reader.read2(&mut buf).await.expect("read2");
                        if n == 0 {
                            break;
                        }
                    }
                });

                task::spawn(async move {
                    let data = b"helloworld";
                    for _ in 0_i32..5 {
                        writer.write_all2(data).await.expect("write_all2");
                    }
                });
            });

            let _client = task::spawn(async move {
                let s = TcpStream::connect(addr).await.expect("connect");

                let (mut reader, mut writer) = s.split2();

                task::spawn(async move {
                    let mut buf = vec![0; 512];
                    loop {
                        let n = reader.read2(&mut buf).await.expect("read2");
                        if n == 0 {
                            break;
                        }
                    }
                });

                task::spawn(async move {
                    let data = b"helloworld";
                    for _ in 0_i32..5 {
                        writer.write_all2(data).await.expect("write_all2");
                    }
                });
            });
        });
    }
}
