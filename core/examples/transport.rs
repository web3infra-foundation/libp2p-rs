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

use async_std::task;
use libp2p_core::pnet::{PnetConfig, PreSharedKey};
use libp2p_core::transport::memory::MemoryTransport;
use libp2p_core::transport::protector::ProtectorTransport;
use libp2p_core::transport::upgrade::TransportUpgrade;
use libp2p_core::transport::{ITransport, TransportListener};
use libp2p_core::upgrade::DummyUpgrader;
use libp2p_core::{Multiaddr, Transport};
use libp2p_traits::{ReadEx, WriteEx};
use log;

fn ttt<T>(trans: ITransport<T>) {}

fn main() {
    env_logger::init();

    // Setup listener.
    let rand_port = rand::random::<u64>().saturating_add(1);
    let t1_addr: Multiaddr = format!("/memory/{}", rand_port).parse().unwrap();

    let listen_addr = t1_addr.clone();
    let psk = "/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683"
        .parse::<PreSharedKey>()
        .unwrap();
    let pnet = PnetConfig::new(psk);
    let pro_trans = ProtectorTransport::new(MemoryTransport::default(), pnet);

    let it = Box::new(pro_trans);
    //ttt(it);

    //let pro_trans = MemoryTransport::default();
    task::spawn(async move {
        let t1 = Box::new(TransportUpgrade::new(pro_trans, DummyUpgrader::new(), DummyUpgrader::new()));
        let mut listener = t1.listen_on(listen_addr).unwrap();

        loop {
            let mut stream = listener.accept().await.unwrap();
            task::spawn(async move {
                stream.write2(b"hello").await?;
                Ok::<(), std::io::Error>(())
            });
        }
    });

    // Setup dialer.
    for i in 0..10u32 {
        let addr = t1_addr.clone();
        task::spawn(async move {
            log::info!("start client{}", i);

            let t2 = TransportUpgrade::new(pro_trans, DummyUpgrader::new(), DummyUpgrader::new());
            let mut socket = t2.dial(addr).await.unwrap();

            let mut msg = [0; 5];
            //socket.write_all2(&msg).await.unwrap();
            socket.read_exact2(&mut msg).await.unwrap();
            log::info!("client{} got {:?}", i, String::from_utf8_lossy(&mut msg));
        });
    }

    loop {}
}
