// Copyright 2019 Parity Technologies (UK) Ltd.
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

use async_trait::async_trait;
use std::fmt;
use log::trace;
use crate::upgrade::{Upgrader, UpgradeInfo};
use crate::transport::{TransportError};

/// Implementation of dummy `Upgrader` that doesn't do anything practice.
///
/// Useful for testing purposes.
pub struct DummyUpgrader;

impl DummyUpgrader {
    /// Builds a new `DummyUpgrader`.
    pub fn new() -> Self {
        DummyUpgrader
    }
}

impl Default for DummyUpgrader {
    fn default() -> Self {
        DummyUpgrader::new()
    }
}

impl fmt::Debug for DummyUpgrader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "DummyUpgrader")
    }
}

impl Clone for DummyUpgrader {
    fn clone(&self) -> Self {
        DummyUpgrader
    }
}

impl UpgradeInfo for DummyUpgrader {
    type Info = &'static [u8];

    fn protocol_info(&self) -> Vec<Self::Info> {
        vec!(b"/dummy/1.0.0")
    }
}


#[async_trait]
impl<T: Send + 'static> Upgrader<T> for DummyUpgrader {
    type Output = T;

    async fn upgrade_inbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        trace!("dummy upgrader, upgrade inbound connection");
        Ok(socket)
    }

    async fn upgrade_outbound(self, socket: T, _info: <Self as UpgradeInfo>::Info) -> Result<Self::Output, TransportError> {
        trace!("dummy upgrader, upgrade outbound connection");
        Ok(socket)
    }
}
