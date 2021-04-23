use log::{debug, trace};
use std::{cmp::min, io, pin::Pin};

use futures::io::{ReadHalf, WriteHalf};

use crate::{codec::Hmac, crypto::BoxStreamCipher, error::SecioError};
use futures::task::{Context, Poll};
use futures::{stream::BoxStream, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Sink, Stream, StreamExt};
use quicksink::Action;
use std::io::Read;

/// Encrypted stream
#[pin_project::pin_project]
pub struct SecureStream<T> {
    #[pin]
    decrypter: Decrypter<ReadHalf<T>>,
    #[pin]
    encrypter: Encrypter<WriteHalf<T>>,

    /// denotes a sequence of bytes which are expected to be
    /// found at the beginning of the stream and are checked for equality
    nonce: Vec<u8>,
}

impl<T> SecureStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    /// New SecureStream
    pub fn new(
        socket: T,
        max_frame_len: usize,
        decode_cipher: BoxStreamCipher,
        decode_hmac: Option<Hmac>,
        encode_cipher: BoxStreamCipher,
        encode_hmac: Option<Hmac>,
        nonce: Vec<u8>,
    ) -> Self {
        let (r, w) = socket.split();
        let decrypter = Decrypter::new(r, max_frame_len, decode_cipher, decode_hmac);
        let encrypter = Encrypter::new(w, max_frame_len, encode_cipher, encode_hmac);

        SecureStream {
            decrypter,
            encrypter,
            nonce,
        }
    }

    /// Verify nonce between local and remote
    pub(crate) async fn verify_nonce(&mut self) -> Result<(), SecioError> {
        if !self.nonce.is_empty() {
            let mut nonce = self.nonce.clone();
            trace!("nonce len: {}", nonce.len());
            self.read_exact(&mut nonce).await?;

            // trace!("verify_nonce nonce={}, my_nonce={}", nonce_len, self.nonce.len());

            let n = min(nonce.len(), self.nonce.len());
            if nonce[..n] != self.nonce[..n] {
                return Err(SecioError::NonceVerificationFailed);
            }
            self.nonce.drain(..n);
            self.nonce.shrink_to_fit();
        }

        Ok(())
    }
}

impl<T> AsyncRead for SecureStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        self.project().decrypter.poll_read(cx, buf)
    }
}

fn map_secio_error_to_io_error(e: SecioError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

impl<T> AsyncWrite for SecureStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.project().encrypter.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().encrypter.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().encrypter.poll_close(cx)
    }
}

#[pin_project::pin_project]
struct Decrypter<R> {
    current_item: Option<std::io::Cursor<Vec<u8>>>,
    #[pin]
    stream: BoxStream<'static, io::Result<Vec<u8>>>,
    decode_cipher: BoxStreamCipher,
    decode_hmac: Option<Hmac>,
    _mark: std::marker::PhantomData<R>,
}

impl<R> Decrypter<R>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    fn new(r: R, max_len: usize, decode_cipher: BoxStreamCipher, decode_hmac: Option<Hmac>) -> Self {
        let stream = futures::stream::unfold(r, move |mut r| async move {
            let mut len = [0; 4];
            if let Err(e) = r.read_exact(&mut len).await {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return None;
                }
                return Some((Err(e), r));
            }
            let n = u32::from_be_bytes(len) as usize;
            if n > max_len {
                let msg = format!("data length {} exceeds allowed maximum {}", n, max_len);
                return Some((Err(io::Error::new(io::ErrorKind::PermissionDenied, msg)), r));
            }
            let mut v = vec![0; n];
            if let Err(e) = r.read_exact(&mut v).await {
                return Some((Err(e), r));
            }
            Some((Ok(v), r))
        });

        Decrypter {
            current_item: None,
            stream: stream.boxed(),
            decode_hmac,
            decode_cipher,
            _mark: std::marker::PhantomData,
        }
    }

    /*
    /// Decoding data
    #[inline]
    fn decode_buffer(self: Pin<&mut Self>, mut frame: Vec<u8>) -> Result<Vec<u8>, SecioError> {
        let this = self.project();
        if let Some(ref mut hmac) = this.decode_hmac {
            if frame.len() < hmac.num_bytes() {
                debug!("frame too short when decoding secio frame");
                return Err(SecioError::FrameTooShort);
            }

            let content_length = frame.len() - hmac.num_bytes();
            {
                let (crypted_data, expected_hash) = frame.split_at(content_length);
                debug_assert_eq!(expected_hash.len(), hmac.num_bytes());

                if !hmac.verify(crypted_data, expected_hash) {
                    debug!("hmac mismatch when decoding secio frame");
                    return Err(SecioError::HmacNotMatching);
                }
            }

            frame.truncate(content_length);
        }

        let out = this.decode_cipher.decrypt(&frame)?;

        Ok(out)
    }
     */
}

impl<R> AsyncRead for Decrypter<R>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut this = self.project();
        let item_to_copy = loop {
            if let Some(ref mut i) = this.current_item {
                if i.position() < i.get_ref().len() as u64 {
                    break i;
                }
            }

            let mut frame = match this.stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => frame,
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(Ok(0)),
            };
            if let Some(ref mut hmac) = this.decode_hmac {
                if frame.len() < hmac.num_bytes() {
                    debug!("frame too short when decoding secio frame");
                    return Poll::Ready(Err(map_secio_error_to_io_error(SecioError::FrameTooShort)));
                }

                let content_length = frame.len() - hmac.num_bytes();
                {
                    let (crypted_data, expected_hash) = frame.split_at(content_length);
                    debug_assert_eq!(expected_hash.len(), hmac.num_bytes());

                    if !hmac.verify(crypted_data, expected_hash) {
                        debug!("hmac mismatch when decoding secio frame");
                        return Poll::Ready(Err(map_secio_error_to_io_error(SecioError::HmacNotMatching)));
                    }
                }

                frame.truncate(content_length);
            }

            let frame = this.decode_cipher.as_mut().decrypt(&frame).map_err(map_secio_error_to_io_error)?;

            *this.current_item = Some(io::Cursor::new(frame));
        };
        // Copy it!
        Poll::Ready(Ok(item_to_copy.read(buf)?))
    }
}

#[pin_project::pin_project]
struct Encrypter<W> {
    #[pin]
    sink: Pin<Box<dyn Sink<Vec<u8>, Error = io::Error> + Send>>,
    encode_hmac: Option<Hmac>,
    encode_cipher: BoxStreamCipher,
    _mark: std::marker::PhantomData<W>,
}

impl<W> Encrypter<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    fn new(w: W, max_len: usize, encode_cipher: BoxStreamCipher, encode_hmac: Option<Hmac>) -> Self {
        let sink = quicksink::make_sink(w, move |mut w, action: Action<Vec<u8>>| async move {
            match action {
                Action::Send(data) => {
                    if data.len() > max_len {
                        log::error!("data length {} exceeds allowed maximum {}", data.len(), max_len)
                    }
                    w.write_all(&(data.len() as u32).to_be_bytes()).await?;
                    w.write_all(&data).await?
                }
                Action::Flush => w.flush().await?,
                Action::Close => {
                    log::info!("close action");
                    w.close().await?
                }
            }
            Ok(w)
        });

        Encrypter {
            sink: Box::pin(sink),
            encode_cipher,
            encode_hmac,
            _mark: std::marker::PhantomData,
        }
    }

    /*
    /// Encoding buffer
    fn encode_buffer(self: Pin<&mut Self>, buf: &[u8]) -> Result<Vec<u8>, SecioError> {
        let this = self.project();
        let mut out = this.encode_cipher.encrypt(buf)?;
        if let Some(ref mut hmac) = this.encode_hmac {
            let signature = hmac.sign(&out[..]);
            out.extend_from_slice(signature.as_ref());
        }
        Ok(out)
    }
     */
}

impl<W> AsyncWrite for Encrypter<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let mut this = self.project();
        futures::ready!(this.sink.as_mut().poll_ready(cx))?;
        let n = buf.len();

        let mut out = this.encode_cipher.as_mut().encrypt(buf).map_err(map_secio_error_to_io_error)?;
        if let Some(ref mut hmac) = this.encode_hmac {
            let signature = hmac.sign(&out[..]);
            out.extend_from_slice(signature.as_ref());
        }
        if let Err(e) = this.sink.as_mut().start_send(out) {
            return Poll::Ready(Err(e));
        }
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().sink.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().sink.poll_close(cx)
    }
}
