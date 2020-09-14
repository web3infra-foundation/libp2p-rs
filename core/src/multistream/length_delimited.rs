// Copyright 2017 Parity Technologies (UK) Ltd.
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

use bytes::{BufMut as _, Bytes, BytesMut};
use std::{convert::TryFrom as __, io, u16};

use super::{ReadEx, WriteEx};

const MAX_LEN_BYTES: u16 = 2;
const MAX_FRAME_SIZE: u16 = (1 << (MAX_LEN_BYTES * 8 - MAX_LEN_BYTES)) - 1;
const DEFAULT_BUFFER_SIZE: usize = 64;

/// A `Stream` and `Sink` for unsigned-varint length-delimited frames,
/// wrapping an underlying `AsyncRead + AsyncWrite` I/O resource.
///
/// We purposely only support a frame sizes up to 16KiB (2 bytes unsigned varint
/// frame length). Frames mostly consist in a short protocol name, which is highly
/// unlikely to be more than 16KiB long.
#[derive(Debug)]
pub struct LengthDelimited<R> {
    /// The inner I/O resource.
    inner: R,
    /// Read buffer for a single incoming unsigned-varint length-delimited frame.
    read_buffer: BytesMut,
    /// Write buffer for outgoing unsigned-varint length-delimited frames.
    write_buffer: BytesMut,
}

impl<R: ReadEx + WriteEx + Send> LengthDelimited<R> {
    /// Creates a new I/O resource for reading and writing unsigned-varint
    /// length delimited frames.
    pub fn new(inner: R) -> LengthDelimited<R> {
        LengthDelimited {
            inner,
            read_buffer: BytesMut::with_capacity(DEFAULT_BUFFER_SIZE),
            write_buffer: BytesMut::with_capacity(DEFAULT_BUFFER_SIZE + MAX_LEN_BYTES as usize),
        }
    }

    /// Drops the [`LengthDelimited`] resource, yielding the underlying I/O stream
    /// together with the remaining write buffer containing the uvi-framed data
    /// that has not yet been written to the underlying I/O stream.
    ///
    /// The returned remaining write buffer may be prepended to follow-up
    /// protocol data to send with a single `write`. Either way, if non-empty,
    /// the write buffer _must_ eventually be written to the I/O stream
    /// _before_ any follow-up data, in order to maintain a correct data stream.
    ///
    /// # Panic
    ///
    /// Will panic if called while there is data in the read buffer. The read buffer is
    /// guaranteed to be empty whenever `Stream::poll` yields a new `Bytes` frame.
    pub fn into_inner(self) -> R {
        self.inner
    }

    async fn read_unsigned_varint(&mut self) -> io::Result<u16> {
        let mut b = unsigned_varint::encode::u16_buffer();
        for i in 0..b.len() {
            self.inner.read_exact2(&mut b[i..i + 1]).await?;
            if unsigned_varint::decode::is_last(b[i]) {
                return Ok(unsigned_varint::decode::u16(&b[..=i])
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                    .0);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            unsigned_varint::decode::Error::Overflow,
        ))
    }

    pub async fn recv_message(&mut self) -> io::Result<Bytes> {
        let len = self.read_unsigned_varint().await?;
        if len > MAX_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Maximum frame length exceeded",
            ));
        }
        let buf = &mut self.read_buffer;
        buf.clear();
        buf.resize(len as usize, 0);
        if len > 0 {
            self.inner.read_exact2(buf).await?;
        }
        Ok(buf.split_off(0).freeze())
    }

    pub async fn send_message(&mut self, buf: Bytes) -> io::Result<()> {
        let len = match u16::try_from(buf.len()) {
            Ok(len) if len <= MAX_FRAME_SIZE => len,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Maximum frame size exceeded.",
                ))
            }
        };
        let mut uvi_buf = unsigned_varint::encode::u16_buffer();
        let uvi_len = unsigned_varint::encode::u16(len, &mut uvi_buf);
        self.write_buffer.clear();
        self.write_buffer.reserve(len as usize + uvi_len.len());
        self.write_buffer.put(uvi_len);
        self.write_buffer.put(buf);
        self.inner.write_all2(&self.write_buffer).await
    }
}

#[cfg(test)]
mod tests {
    use super::LengthDelimited;
    use async_std::net::{TcpListener, TcpStream};
    use futures::{io::Cursor, prelude::*};
    use quickcheck::*;
    use std::io::ErrorKind;
    // use super::{ReadEx, WriteEx};

    #[test]
    fn basic_read() {
        let data = vec![6, 9, 8, 7, 6, 5, 4];
        let framed = LengthDelimited::new(Cursor::new(data));
        let s = futures::stream::unfold(framed, |mut f| async move {
            f.recv_message().await.map(|buf| (buf, f)).ok()
        });
        let recved = futures::executor::block_on(s.collect::<Vec<_>>());
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4]]);
    }

    #[test]
    fn basic_read_two() {
        let data = vec![6, 9, 8, 7, 6, 5, 4, 3, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data));
        let s = futures::stream::unfold(framed, |mut f| async move {
            f.recv_message().await.map(|buf| (buf, f)).ok()
        });
        let recved = futures::executor::block_on(s.collect::<Vec<_>>());
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4], vec![9, 8, 7]]);
    }

    #[test]
    fn two_bytes_long_packet() {
        let len = 5000u16;
        assert!(len < (1 << 15));
        let frame = (0..len).map(|n| (n & 0xff) as u8).collect::<Vec<_>>();
        let mut data = vec![(len & 0x7f) as u8 | 0x80, (len >> 7) as u8];
        data.extend(frame.clone().into_iter());
        let mut framed = LengthDelimited::new(Cursor::new(data));
        let recved =
            futures::executor::block_on(async move { framed.recv_message().await }).unwrap();
        assert_eq!(recved, frame);
    }

    #[test]
    fn packet_len_too_long() {
        let mut data = vec![0x81, 0x81, 0x1];
        data.extend((0..16513).map(|_| 0));
        let mut framed = LengthDelimited::new(Cursor::new(data));
        let recved = futures::executor::block_on(async move { framed.recv_message().await });

        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::InvalidData)
        } else {
            panic!()
        }
    }

    #[test]
    fn empty_frames() {
        let data = vec![0, 0, 6, 9, 8, 7, 6, 5, 4, 0, 3, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data));
        let s = futures::stream::unfold(framed, |mut f| async move {
            f.recv_message().await.map(|buf| (buf, f)).ok()
        });
        let recved = futures::executor::block_on(s.collect::<Vec<_>>());
        assert_eq!(
            recved,
            vec![
                vec![],
                vec![],
                vec![9, 8, 7, 6, 5, 4],
                vec![],
                vec![9, 8, 7],
            ]
        );
    }

    #[test]
    fn unexpected_eof_in_len() {
        let data = vec![0x89];
        let framed = LengthDelimited::new(Cursor::new(data));
        let s = futures::stream::try_unfold(framed, |mut f| async move {
            f.recv_message().await.map(|buf| Some((buf, f)))
        });
        let recved = futures::executor::block_on(s.try_collect::<Vec<_>>());
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data() {
        let data = vec![5];
        let framed = LengthDelimited::new(Cursor::new(data));
        let framed = futures::stream::try_unfold(framed, |mut f| async move {
            f.recv_message().await.map(|buf| Some((buf, f)))
        });
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>());
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data2() {
        let data = vec![5, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data));
        let framed = futures::stream::try_unfold(framed, |mut f| async move {
            f.recv_message().await.map(|buf| Some((buf, f)))
        });
        let recved = futures::executor::block_on(framed.try_collect::<Vec<_>>());
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn writing_reading() {
        fn prop(frames: Vec<Vec<u8>>) -> TestResult {
            async_std::task::block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let listener_addr = listener.local_addr().unwrap();

                let expected_frames = frames.clone();
                let server = async_std::task::spawn(async move {
                    let socket = listener.accept().await.unwrap().0;

                    let mut framed = LengthDelimited::new(socket);
                    /*
                    let framed = futures::stream::try_unfold(framed, |mut f| async move {
                        f.recv_message().await.map(|buf| Some((buf, f)))
                    });
                    let mut connec = rw_stream_sink::RwStreamSink::new(framed);
                     */

                    for expected in expected_frames {
                        log::info!("recv message");
                        let b = framed.recv_message().await.unwrap();
                        assert_eq!(&b, &expected[..]);
                    }
                });

                let client = async_std::task::spawn(async move {
                    let socket = TcpStream::connect(&listener_addr).await.unwrap();
                    let mut connec = LengthDelimited::new(socket);
                    for frame in frames {
                        log::info!("send message");
                        connec.send_message(From::from(frame)).await.unwrap();
                    }
                });

                server.await;
                client.await;
            });

            TestResult::passed()
        }

        quickcheck(prop as fn(_) -> _)
    }
}
