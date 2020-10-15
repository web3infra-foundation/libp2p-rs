use libp2p_traits::{ReadEx, WriteEx};
use std::io::{self, ErrorKind};

const U32_LEN: usize = 5;

pub struct LengthDelimited<T> {
    inner: T,
    max_frame_size: u32,
}

impl<R> LengthDelimited<R>
where
    R: Unpin + Send,
{
    /// Creates a new I/O resource for reading and writing unsigned-varint
    /// length delimited frames.
    pub fn new(inner: R, max_frame_size: u32) -> LengthDelimited<R> {
        LengthDelimited { inner, max_frame_size }
    }
}

impl<T> LengthDelimited<T>
where
    T: ReadEx,
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
        for (pos, _) in (0..U32_LEN).enumerate() {
            self.read_byte(&mut buf[pos..pos + 1]).await?;
            if buf[pos] < 0x80 {
                // MSB is not set, indicating the end of the length prefix.
                let (len, _) = unsigned_varint::decode::u32(&buf).map_err(|e| {
                    log::debug!("invalid length prefix: {}", e);
                    io::Error::new(io::ErrorKind::InvalidData, "invalid length prefix")
                })?;
                return Ok(len);
            }
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
    T: WriteEx,
{
    pub async fn write_header(&mut self, hdr: u32) -> io::Result<()> {
        let mut uvi_buf = unsigned_varint::encode::u32_buffer();
        let header_byte = unsigned_varint::encode::u32(hdr, &mut uvi_buf);
        self.inner.write_all2(header_byte).await
    }

    pub async fn write_length(&mut self, length: u32) -> io::Result<()> {
        if length > self.max_frame_size {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Maximum frame size exceeded."));
        }

        let mut uvi_buf = unsigned_varint::encode::u32_buffer();
        let uvi_len = unsigned_varint::encode::u32(length, &mut uvi_buf);
        self.inner.write_all2(uvi_len).await
    }

    pub async fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
        self.inner.write_all2(data).await
    }
}

impl<T> LengthDelimited<T>
where
    T: WriteEx + Send,
{
    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        self.inner.flush2().await
    }

    pub(crate) async fn close(&mut self) -> io::Result<()> {
        self.inner.close2().await
    }
}
