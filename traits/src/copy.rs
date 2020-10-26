// Copyright 2020 Netwarps Ltd.
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

use crate::{ReadEx, WriteEx};
/// Copies the entire contents of a reader into a writer.
///
/// This function will read data from `reader` and write it into `writer` in a streaming fashion
/// until `reader` returns EOF.
///
/// On success, returns the total number of bytes copied.
///
pub async fn copy<R, W>(mut reader: R, mut writer: W) -> std::io::Result<usize>
where
    R: ReadEx + Unpin,
    W: WriteEx + Unpin,
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
