
**Note: Obsolete**. We have decided to remove ReadEx/WriteEx/SplitEx in near future. We realize they are not compatible with the existing AsyncRead/AsyncWrite, so the I/O object which support ReadEx/WriteEx can not be applied to the existing code/lib. Moreover, SplitEx is kind of complicated, bringing more constraints to the generic types, which is bad. So... it is an unsuccessful attempt, but still interesting...

**Updated on 22 Feb, 2022: 
ReadEx/WriteEx has been removed in release v0.3.0, so all related docs will be marked as deleted.**

# ~~ReadEx and WriteEx~~

~~ReadEx and WriteEx are the traits to support I/O operations, quite similar to the AsyncRead and AsyncWrite combination defined in futures::io. As for the latter, the corresponding Ext traits provide read/write futures that can be `await`ed by async code. But for developers, we have to implement the `poll_xxx` methods if we want to implement AsyncRead and AsyncWrite. It is so called implementing future manually. Actually, ReadEx and WriteEx provide the similar functionanlity as they can be `await`ed as well, however, not like the AsyncRead + AsyncWrite, these two are async traits which allow to directly write async fn() in traits. Thus, we don't have to write `poll_xxx` methods any more. This is the motivation of introducing these two traits.~~

~~The async methods in ReadEx and WriteEx are almost the async cloned version of thoese in AsyncRead and AsyncWrite, f.g. AsyncRead::read => ReadEx::read2. In addition, ReadEx and WriteEx also provide some convenience by adding a few default implementations of fixed or variant length-prefix helper methods.~~

~~ReadEx and WriteEx are defined as below:~~

[comment]: <> (```no_run)

[comment]: <> (/// Read Trait for async/wait)

[comment]: <> (///)

[comment]: <> (#[async_trait])

[comment]: <> (pub trait ReadEx: Send {)

[comment]: <> (    async fn read2&#40;&mut self, buf: &mut [u8]&#41; -> Result<usize, io::Error>;)

[comment]: <> (    async fn read_exact2<'a>&#40;&'a mut self, buf: &'a mut [u8]&#41; -> Result<&#40;&#41;, io::Error> { ... })

[comment]: <> (    async fn read_fixed_u32&#40;&mut self&#41; -> Result<usize, io::Error> { ... })

[comment]: <> (    async fn read_varint&#40;&mut self&#41; -> Result<usize, io::Error> { ... })

[comment]: <> (    async fn read_one&#40;&mut self, max_size: usize&#41; -> Result<Vec<u8>, io::Error> { ... })

[comment]: <> (})

[comment]: <> (/// Write Trait for async/wait)

[comment]: <> (///)

[comment]: <> (#[async_trait])

[comment]: <> (pub trait WriteEx: Send {)

[comment]: <> (    async fn write2&#40;&mut self, buf: &[u8]&#41; -> Result<usize, io::Error>;)

[comment]: <> (    async fn write_all2&#40;&mut self, buf: &[u8]&#41; -> Result<&#40;&#41;, io::Error> { ... })

[comment]: <> (    async fn write_varint&#40;&mut self, len: usize&#41; -> Result<&#40;&#41;, io::Error> { ... })

[comment]: <> (    async fn write_fixed_u32&#40;&mut self, len: usize&#41; -> Result<&#40;&#41;, io::Error> { ... })

[comment]: <> (    async fn write_one&#40;&mut self, buf: &[u8]&#41; -> Result<&#40;&#41;, io::Error> { ... })

[comment]: <> (    async fn flush2&#40;&mut self&#41; -> Result<&#40;&#41;, io::Error>;)

[comment]: <> (    async fn close2&#40;&mut self&#41; -> Result<&#40;&#41;, io::Error>;)

[comment]: <> (})

[comment]: <> (```)

~~In general, all I/O objects in `libp2p-rs` support ReadEx/WriteEx, furthermore in Swarm, A trait object of ReadEx + WriteEx, called 'IReadWrite', is used to construct the Substream which contains a raw substream opened by stream muxer.~~

~~In order to integrate with the standard AsyncRead and AsyncWrite, ReadEx and WriteEx are automatically implemented for any types which implement AsyncRead and AsyncWrite respectively.~~

[comment]: <> (```no_run)

[comment]: <> (#[async_trait])

[comment]: <> (impl<T: AsyncRead + Unpin + Send> ReadEx for T {)

[comment]: <> (    async fn read2&#40;&mut self, buf: &mut [u8]&#41; -> Result<usize, io::Error> {)

[comment]: <> (        let n = AsyncReadExt::read&#40;self, buf&#41;.await?;)

[comment]: <> (        Ok&#40;n&#41;)

[comment]: <> (    })

[comment]: <> (})

[comment]: <> (#[async_trait])

[comment]: <> (impl<T: AsyncWrite + Unpin + Send> WriteEx for T {)

[comment]: <> (    async fn write2&#40;&mut self, buf: &[u8]&#41; -> Result<usize, io::Error> {)

[comment]: <> (        AsyncWriteExt::write&#40;self, buf&#41;.await)

[comment]: <> (    })

[comment]: <> (    async fn flush2&#40;&mut self&#41; -> Result<&#40;&#41;, io::Error> {)

[comment]: <> (        AsyncWriteExt::flush&#40;self&#41;.await)

[comment]: <> (    })

[comment]: <> (    async fn close2&#40;&mut self&#41; -> Result<&#40;&#41;, io::Error> {)

[comment]: <> (        AsyncWriteExt::close&#40;self&#41;.await)

[comment]: <> (    })

[comment]: <> (})

[comment]: <> (```)

## ~~SplitEx~~
~~As you know, there is a split() method in AsyncReadExt to support splitting T: AsyncRead + AsyncWrite to a ReadHalf and WriteHalf pair, so that they can be handled/moved separately. Unfortunately We haven't figured out a proper way to add `split` method to a type which supports ReadEx + WriteEx, as AsyncRead/AsyncWrite does. However, it is absolutely important for Yamux/Mplex to be able to split an underlying connection and then handle the receiving procedure properly. More details about it please check the code of Yamux Connection.~~

~~As a workaround, we introduce this trait to solve the problem. In order to support splitting, a I/O connection/socket has to support `SplitEx` trait. As a result, the secure stream layer is required to implement `SplitEx`, which can be later used by the stream muxer layer.~~

~~And for any T which supports AsyncRead and AsyncWrite, SplitEx will be implemeted automatically. This is to say, a TcpStream will have `SplitEx`:~~

[comment]: <> (```no_run)

[comment]: <> (impl<T: AsyncRead + AsyncWrite + Send + Unpin> SplitEx for T {)

[comment]: <> (    type Reader = ReadHalf<T>;)

[comment]: <> (    type Writer = WriteHalf<T>;)

[comment]: <> (    fn split&#40;self&#41; -> &#40;Self::Reader, Self::Writer&#41; {)

[comment]: <> (        futures::AsyncReadExt::split&#40;self&#41;)

[comment]: <> (    })

[comment]: <> (})

[comment]: <> (```)

[comment]: <> (> Note: `SplitEx` is only required by Stream Muxer layer. As for Swarm::Substream, which supports `Clone`, we don't apply `SplitEx` at all.)

## ~~Issue about AsyncTrait default implementation~~

~~Both ReadEx and WriteEx include methods with default implementations. As required by async-trait, they can be made into a trait object only when they are derived from `Send`, since default implemetations requires `Self` bound. This is why these two traits both derive from `Send`. More details please check async-trait Readme.~~


