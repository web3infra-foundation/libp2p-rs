use crate::{Read2, Write2};
/// Copies the entire contents of a reader into a writer.
///
/// This function will read data from `reader` and write it into `writer` in a streaming fashion
/// until `reader` returns EOF.
///
/// On success, returns the total number of bytes copied.
///
pub async fn copy<R, W>(mut reader: R, mut writer: W) -> std::io::Result<usize>
where
    R: Read2 + Unpin,
    W: Write2 + Unpin + Send,
{
    let mut copyed = 0_usize;

    let mut buf = [0u8; 1024];
    loop {
        let n = reader.read2(&mut buf).await?;
        if n == 0 {
            return Ok(copyed);
        }
        copyed += n;
        writer.write_all2(&buf[..n]).await?;
    }
}
