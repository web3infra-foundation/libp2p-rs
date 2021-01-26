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

macro_rules! cfg_async_std {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "async-std")]
            $item
        )*
    }
}

macro_rules! cfg_tokio {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "tokio")]
            $item
        )*
    }
}

cfg_async_std! {
    mod async_std;
    pub use self::async_std::{task, net};
}

cfg_tokio! {
    mod tokio;
    pub use self::tokio::*;
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_one() {}
}
