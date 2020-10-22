
# ReadEx and WriteEx

ReadEx and WriteEx are the traits to support I/O operations, quite similar to the AsyncRead and AsyncWrite combination defined in futures::io. As for the latter, the corresponding Ext traits provide read/write futures that can be `await`ed by async code. But for developers, we have to implement the `poll_xxx` methods if we want to implement AsyncRead and AsyncWrite. It is so called implementing future manually. Actually, ReadEx and WriteEx provide the similar functionanlity as they can be `await`ed as well, however, not like the AsyncRead + AsyncWrite, these two are async traits which allow to directly write async fn() in traits. Thus, we don't have to write `poll_xxx` methods any more. This is the motivation of introducing these two traits.

The async methods in ReadEx and WriteEx are almost the async cloned version of thoese in AsyncRead and AsyncWrite, f.g. AsyncRead::read => ReadEx::read2. In addition, ReadEx and WriteEx also provide some convenience by adding a few default implementations of fixed or variant length-prefix helper methods.

ReadEx and WriteEx are defined as below:

```no_run

/// Read Trait for async/wait
///
#[async_trait]
pub trait ReadEx: Send {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error>;
    async fn read_exact2<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<(), io::Error> { ... }
    async fn read_fixed_u32(&mut self) -> Result<usize, io::Error> { ... }
    async fn read_varint(&mut self) -> Result<usize, io::Error> { ... }
    async fn read_one(&mut self, max_size: usize) -> Result<Vec<u8>, io::Error> { ... }
}

/// Write Trait for async/wait
///
#[async_trait]
pub trait WriteEx: Send {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error>;
    async fn write_all2(&mut self, buf: &[u8]) -> Result<(), io::Error> { ... }
    async fn write_varint(&mut self, len: usize) -> Result<(), io::Error> { ... }
    async fn write_fixed_u32(&mut self, len: usize) -> Result<(), io::Error> { ... }
    async fn write_one(&mut self, buf: &[u8]) -> Result<(), io::Error> { ... }
    async fn flush2(&mut self) -> Result<(), io::Error>;
    async fn close2(&mut self) -> Result<(), io::Error>;
}
```

In general, all I/O objects in `libp2p-rs` support ReadEx/WriteEx, furthermore in Swarm, A trait object of ReadEx + WriteEx, called 'IReadWrite', is used to construct the Substream which contains a raw substream opened by stream muxer.

In order to integrate with the standard AsyncRead and AsyncWrite, ReadEx and WriteEx are automatically implemented for any types which implement AsyncRead and AsyncWrite respectively.

```no_run
#[async_trait]
impl<T: AsyncRead + Unpin + Send> ReadEx for T {
    async fn read2(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        let n = AsyncReadExt::read(self, buf).await?;
        Ok(n)
    }
}

#[async_trait]
impl<T: AsyncWrite + Unpin + Send> WriteEx for T {
    async fn write2(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        AsyncWriteExt::write(self, buf).await
    }

    async fn flush2(&mut self) -> Result<(), io::Error> {
        AsyncWriteExt::flush(self).await
    }

    async fn close2(&mut self) -> Result<(), io::Error> {
        AsyncWriteExt::close(self).await
    }
}
```


## Issues

Both ReadEx and WriteEx include methods with default implementations. As required by async-trait, they can be made into a trait object only when they are derived from `Send`, since default implemetations requires `Self` bound. This is why these two traits both derive from `Send`. More details please check async-trait Readme.

We haven't figured out a proper way to add `split` method to a type which supports ReadEx + WriteEx, as AsyncRead/AsyncWrite does. However, it is quite important as most likely we 'd like to split an I/O object into the ReadHalf and WriteHalf pair. There is probably a wrong implementation of 'split' at this moment. We'll look into it later... 

