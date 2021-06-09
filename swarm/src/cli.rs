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

use std::str::FromStr;
use xcli::*;

use libp2prs_core::{Multiaddr, PeerId};
use libp2prs_runtime::task;

use crate::Control;

const SWRM: &str = "swarm";

pub fn swarm_cli_commands<'a>() -> Command<'a> {
    Command::new_with_alias(SWRM, "s")
        .about("Swarm")
        .usage("swarm")
        .subcommand(Command::new("close").about("close swarm").usage("close").action(cli_close_swarm))
        .subcommand(Command::new("id").about("show id information").usage("id").action(cli_show_id))
        .subcommand(
            Command::new_with_alias("stats", "st")
                .about("dump statistics")
                .usage("stats")
                .action(cli_dump_statistics),
        )
        .subcommand(
            Command::new_with_alias("connection", "lc")
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
            Command::new_with_alias("connect", "con")
                .about("connect to a peer")
                .usage("connect <PeerId> [Multiaddr]")
                .action(cli_connect),
        )
        .subcommand(
            Command::new_with_alias("disconnect", "disc")
                .about("disconnect a peer")
                .usage("disconnect <PeerId>")
                .action(cli_disconnect),
        )
        .subcommand(
            Command::new_with_alias("latency", "lat")
                .about("display latency that connected to each peer")
                .usage("latency")
                .action(cli_dump_peer_latency),
        )
        .subcommand(
            Command::new_with_alias("rate", "r")
                .about("display rate that connected to each peer")
                .usage("rate")
                .action(cli_dump_rate),
        )
}

fn handler(app: &App) -> Control {
    let value_any = app.get_handler(SWRM).expect(SWRM);
    let swarm = value_any.downcast_ref::<Control>().expect("control").clone();
    swarm
}

fn cli_show_id(app: &App, _args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);
    task::block_on(async {
        if let Ok(id) = swarm.retrieve_networkinfo().await {
            println!("Id                : {}", id.id);
            println!("Total Peers       : {:?}", id.num_peers);
            println!("Total Connections : {:?}", id.num_connections);
            println!("Total Substreams  : {:?}", id.num_active_streams);
        }

        if let Ok(identify) = swarm.retrieve_identify_info().await {
            println!("PublicKey         : {:?}", identify.public_key);
            println!("Protocol Version  : {:?}", identify.protocol_version);
            println!("Agent Version     : {:?}", identify.agent_version);
            println!("Support Protocols : {:?}", identify.protocols);
            println!("Listening address : {:?}", identify.listen_addrs);
        }

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

    swarm.close();
    Ok(CmdExeCode::Ok)
}

fn cli_dump_statistics(app: &App, _args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    task::block_on(async {
        let stats = swarm.dump_statistics().await.unwrap();
        println!("{:?}", stats.base);
        println!("{:?}", stats.dialer);
        println!("{:?}", stats.listener);
    });

    Ok(CmdExeCode::Ok)
}

fn cli_dump_peer_latency(app: &App, _args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    task::block_on(async {
        let info = swarm.dump_latency().await.unwrap();
        println!("Remote-Peer-Id                                       Latency");
        info.iter().for_each(|item| println!("{:52} {:?}", item.0, item.1));
    });
    Ok(CmdExeCode::Ok)
}

fn cli_dump_rate(app: &App, _args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    task::block_on(async {
        let info = swarm.dump_rate().await.unwrap();
        println!("Received-rates Send-rates");
        println!("{:<14} {:?}", info.0, info.1);
    });
    Ok(CmdExeCode::Ok)
}

fn cli_show_connections(app: &App, args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    let peer = match args.len() {
        0 => None,
        1 => Some(PeerId::from_str(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?),
        _ => return Err(XcliError::MismatchArgument(1, args.len())),
    };

    task::block_on(async {
        let connections = swarm.dump_connections(peer).await.unwrap();
        println!("CID   DIR Remote-Peer-Id                                       I/O Local-Multiaddr                  Remote-Multiaddr                 Latency");
        connections.iter().for_each(|v| {
            println!(
                "{} {} {:52} {}/{} {: <32} {: <32} {:?} // {} {}",
                v.id,
                v.dir,
                v.info.remote_peer_id,
                v.info.num_inbound_streams,
                v.info.num_outbound_streams,
                v.info.la.to_string(),
                v.info.ra.to_string(),
                v.route_trip_time,
                v.rate_in,
                v.rate_out
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
        1 => Some(PeerId::from_str(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?),
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

    let (peer_id, opt_addr) = match args.len() {
        0 => return Err(XcliError::MissingArgument),
        1 => (PeerId::from_str(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?, None),
        2 => (
            PeerId::from_str(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?,
            Some(Multiaddr::from_str(args[1]).map_err(|e| XcliError::BadArgument(e.to_string()))?),
        ),
        _ => return Err(XcliError::MismatchArgument(1, args.len())),
    };

    task::block_on(async {
        let r = if let Some(addr) = opt_addr {
            swarm.connect_with_addrs(peer_id, vec![addr]).await
        } else {
            swarm.new_connection(peer_id).await
        };
        println!("Connecting to {}: {:?}", peer_id, r);
    });

    Ok(CmdExeCode::Ok)
}

fn cli_disconnect(app: &App, args: &[&str]) -> XcliResult {
    let mut swarm = handler(app);

    let peer_id = if args.len() == 1 {
        PeerId::from_str(args[0]).map_err(|e| XcliError::BadArgument(e.to_string()))?
    } else {
        return Err(XcliError::MismatchArgument(1, args.len()));
    };

    task::block_on(async {
        let r = swarm.disconnect(peer_id).await;
        println!("Disconnecting {}: {:?}", peer_id, r);
    });

    Ok(CmdExeCode::Ok)
}
