use bytes::{BufMut, BytesMut};
use libp2p_traits::{Read2, Write2};
use std::convert::TryFrom;
use std::io::{self, ErrorKind};

const U32_LEN: usize = 5;

pub struct LengthDelimited<T> {
    inner: T,
    max_frame_size: usize,
    write_buffer: BytesMut,
}

impl<R> LengthDelimited<R>
where
    R: Unpin + Send,
{
    /// Creates a new I/O resource for reading and writing unsigned-varint
    /// length delimited frames.
    pub fn new(inner: R, max_frame_size: usize) -> LengthDelimited<R> {
        LengthDelimited {
            inner,
            max_frame_size,
            write_buffer: BytesMut::with_capacity(max_frame_size as usize),
        }
    }
}

impl<T> LengthDelimited<T>
where
    T: Read2 + Send,
{
    pub async fn read_byte(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let n = self.inner.read2(buf).await?;
        if n == 1 {
            return Ok(());
        }

        Err(ErrorKind::UnexpectedEof.into())
    }

    pub async fn read_uvarint(&mut self) -> io::Result<u32> {
        let mut buf: [u8; U32_LEN] = [0; U32_LEN];
        let mut pos = 0;
        for _ in 0..U32_LEN {
            self.read_byte(&mut buf[pos..pos + 1]).await?;
            if buf[pos] < 0x80 {
                // MSB is not set, indicating the end of the length prefix.
                let (len, _) = unsigned_varint::decode::u32(&buf).map_err(|e| {
                    log::debug!("invalid length prefix: {}", e);
                    io::Error::new(io::ErrorKind::InvalidData, "invalid length prefix")
                })?;
                return Ok(len);
            }
            pos += 1;
        }
        Ok(0)
    }

    pub async fn read_body(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact2(buf).await?;
        Ok(())
    }
}

impl<T> LengthDelimited<T>
where
    T: Write2 + Send,
{
    pub async fn write_header(&mut self, hdr: u32) -> io::Result<()> {
        let mut uvi_buf = unsigned_varint::encode::u32_buffer();
        let header_byte = unsigned_varint::encode::u32(hdr, &mut uvi_buf);
        self.write_buffer.reserve(header_byte.len());
        self.write_buffer.put(header_byte);

        Ok(())
    }

    pub async fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
        let len = match u32::try_from(data.len()) {
            Ok(len) if len <= self.max_frame_size as u32 => len,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Maximum frame size exceeded.",
                ))
            }
        };

        // assemble
        let mut uvi_buf = unsigned_varint::encode::u32_buffer();
        let uvi_len = unsigned_varint::encode::u32(len, &mut uvi_buf);
        self.write_buffer.reserve(len as usize + uvi_len.len());
        self.write_buffer.put(uvi_len);
        self.write_buffer.put(data);

        Ok(())
    }
}

impl<T> LengthDelimited<T>
where
    T: Write2 + Send,
{
    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        self.inner.write_all2(&self.write_buffer).await?;
        self.inner.flush2().await?;
        self.write_buffer.clear();
        Ok(())
    }

    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.inner.close2().await
    }
}
