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

use std::convert::TryFrom;
use xcli::*;

use libp2prs_core::PeerId;
use libp2prs_runtime::task;

use crate::Control;

const SWRM: &str = "swarm";

pub fn swarm_cli_commands<'a>() -> Command<'a> {
    Command::new_with_alias(SWRM, "s")
        .about("Swarm")
        .usage("swarm")
        .subcommand(Command::new("close").about("close swarm").usage("close").action(cli_close_swarm))
        .subcommand(
            Command::new("show")
                .about("show basic information")
                .usage("show")
                .action(cli_show_basic),
        )
        .subcommand(
            Command::new_with_alias("connection", "con")
                .about("display connection information")
                .usage("connection [PeerId]")
                .action(cli_show_connections),
        )
        .subcommand(
            Command::new_with_alias("peer", "pr")
                .about("display peer information")
                .usage("peer [PeerId]")
                .action(cli_show_peers),
        )
        .subcommand(
            Command::new_with_alias("connect", "c")
                .about("connect to a peer")
                .usage("connect <PeerId> [Multiaddr]")
                .action(cli_connect),
        )
}

fn handler(app: &App) -> Control {
    let value_any = app.get_handler(SWRM).expect(SWRM);
    let swarm = value_any.downcast_ref::<Control>().expect("control").clone();
    swarm
}

fn cli_show_basic(app: &App, _args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);
    task::block_on(async {
        let r = swarm.retrieve_networkinfo().await.unwrap();
        println!("NetworkInfo: {:?}", r);

        let r = swarm.retrieve_identify_info().await.unwrap();
        println!("IdentifyInfo: {:?}", r);

        let addresses = swarm.self_addrs().await.unwrap();
        println!("Addresses: {:?}", addresses);

        println!(
            "Metric: {:?} {:?}",
            swarm.get_recv_count_and_size(),
            swarm.get_sent_count_and_size()
        );
    });

    Ok(CmdExeCode::Ok)
}

fn cli_close_swarm(app: &App, _args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    task::block_on(swarm.close());
    Ok(CmdExeCode::Ok)
}

fn cli_show_connections(app: &App, args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    let peer = match args.len() {
        0 => None,
        1 => Some(PeerId::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?),
        _ => return Err(XcliError::MismatchArgument(1, args.len())),
    };

    task::block_on(async {
        let connections = swarm.dump_connections(peer).await.unwrap();
        println!("CID   DIR Remote-Peer-Id                                       I/O  Remote-Multiaddr");
        connections.iter().for_each(|v| {
            println!(
                "{} {} {:52} {}/{}  {}",
                v.id, v.dir, v.info.remote_peer_id, v.info.num_inbound_streams, v.info.num_outbound_streams, v.info.ra
            );
            if true {
                v.substreams.iter().for_each(|s| {
                    println!("      ({})", s);
                });
            }
        });
    });

    Ok(CmdExeCode::Ok)
}

fn cli_show_peers(app: &App, args: &[&str]) -> XcliResult {
    let swarm = handler(app);

    let pid = match args.len() {
        0 => None,
        1 => Some(PeerId::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?),
        _ => return Err(XcliError::MismatchArgument(1, args.len())),
    };

    if let Some(peer) = pid {
        let addrs = swarm.get_addrs(&peer);
        let protos = swarm.get_protocols(&peer);
        println!("Addrs: {:?}\nProtocols: {:?}", addrs, protos);
    } else {
        let peers = swarm.get_peers();
        println!("Remote-Peer-Id                                       Pin   Multiaddrs");
        peers.iter().for_each(|v| {
            println!("{:52} {:5} {:?}", v, swarm.pinned(v), swarm.get_addrs(v));
        });
    }
    Ok(CmdExeCode::Ok)
}

fn cli_connect(app: &App, args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    let peer_id = if args.len() == 1 {
        PeerId::try_from(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?
    } else {
        return Err(XcliError::MismatchArgument(1, args.len()));
    };

    task::block_on(async {
        let r = swarm.new_connection(peer_id.clone()).await;
        println!("Connecting to {}: {:?}", peer_id, r);
    });

    Ok(CmdExeCode::Ok)
}
