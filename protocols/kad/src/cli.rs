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
use std::str::FromStr;

use crate::Control;
use libp2prs_core::{Multiaddr, PeerId};
use xcli::*;

const DHT: &str = "dht";

pub fn dht_cli_commands<'a>() -> Command<'a> {
    let close_cmd = Command::new("close")
        .about("Close Kad main loop")
        .usage("close")
        .action(cli_close_kad);
    let bootstrap_cmd = Command::new_with_alias("bootstrap", "boot")
        .about("Show or edit the list of bootstrap peers")
        .usage("bootstrap")
        .action(cli_bootstrap);
    let add_node_cmd = Command::new("add")
        .about("Add peer to KBucket")
        .usage("add [<peer>] [<multi_address>]")
        .action(cli_add_node);
    let rm_node_cmd = Command::new("rm")
        .about("Remove peer from KBucket")
        .usage("rm [<peer>] [<multi_address>]")
        .action(cli_rm_node);

    let find_peer_cmd = Command::new_with_alias("findpeer", "fp")
        .about("find peer through dht")
        .usage("findpeer <peerid>")
        .action(find_peer);
    let get_value_cmd = Command::new_with_alias("getvalue", "gv")
        .about("get value through dht")
        .usage("getvalue <key>")
        .action(get_value);

    let dump_kbucket_cmd = Command::new_with_alias("dump", "dp")
        .about("dump k-buckets")
        .usage("dump")
        .action(cli_dump_kbuckets);
    let dump_messenger_cmd = Command::new_with_alias("messenger", "ms")
        .about("dump messengers")
        .usage("messengers")
        .action(cli_dump_messengers);
    let dump_stats_cmd = Command::new_with_alias("stats", "st")
        .about("dump statistics")
        .usage("stats")
        .action(cli_dump_statistics);

    Command::new_with_alias(DHT, "d")
        .about("Kad-DHT")
        .usage("dht")
        .subcommand(close_cmd)
        .subcommand(bootstrap_cmd)
        .subcommand(add_node_cmd)
        .subcommand(rm_node_cmd)
        .subcommand(dump_kbucket_cmd)
        .subcommand(dump_messenger_cmd)
        .subcommand(dump_stats_cmd)
        .subcommand(find_peer_cmd)
        .subcommand(get_value_cmd)
}

fn handler(app: &App) -> Control {
    let value_any = app.get_handler(DHT).expect(DHT);
    let kad = value_any.downcast_ref::<Control>().expect("control").clone();
    kad
}

fn cli_close_kad(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);
    task::block_on(kad.close());
    Ok(CmdExeCode::Ok)
}

fn cli_bootstrap(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);
    task::block_on(async {
        kad.bootstrap().await;
        println!("start to bootstrap");
    });

    Ok(CmdExeCode::Ok)
}

fn cli_add_node(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 2 {
        return Err(XcliError::MismatchArgument(2, args.len()));
    }

    let pid = args.get(0).unwrap();
    let addr = args.get(1).unwrap();

    let peer = PeerId::from_str(pid).map_err(|e| XcliError::BadArgument(e.to_string()))?;
    let address = Multiaddr::from_str(addr).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    task::block_on(async {
        kad.add_node(peer, vec![address]).await;
        println!("add node completed");
    });

    Ok(CmdExeCode::Ok)
}

fn cli_rm_node(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let pid = args.get(0).unwrap();
    let peer = PeerId::from_str(pid).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    task::block_on(async {
        kad.remove_node(peer).await;
        println!("remove node completed");
    });

    Ok(CmdExeCode::Ok)
}

fn cli_dump_kbuckets(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    let verbose = !args.is_empty();

    task::block_on(async {
        let buckets = kad.dump_kbuckets().await.unwrap();
        println!("Index Entries Active");
        for b in buckets {
            let active = b.bucket.iter().filter(|e| e.aliveness.is_some()).count();
            println!("{:<5} {:<7} {}", b.index, b.bucket.len(), active);
            if verbose {
                for p in b.bucket {
                    println!("      {}", p);
                }
            }
        }
    });

    Ok(CmdExeCode::Ok)
}

fn cli_dump_statistics(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    task::block_on(async {
        let stats = kad.dump_statistics().await.unwrap();
        println!("Total refreshes : {}", stats.total_refreshes);
        println!("Successful queries   : {}", stats.successful_queries);
        println!("Timeout queries   : {}", stats.timeout_queries);
        println!("Query details   : {:?}", stats.query);
        println!("Kad rx messages : {:?}", stats.message_rx);
    });

    Ok(CmdExeCode::Ok)
}

fn cli_dump_messengers(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    task::block_on(async {
        let messengers = kad.dump_messengers().await.unwrap();
        println!("Remote-Peer-Id                                       Reuse CID   SID    DIR Protocol");
        for m in messengers {
            println!("{:52} {:<5} {}", m.peer, m.reuse, m.stream);
        }
    });

    Ok(CmdExeCode::Ok)
}

fn get_value(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let key = args.get(0).cloned().unwrap();

    task::block_on(async {
        let value = kad.get_value(Vec::from(key)).await;
        println!("get value: {:?}", value);
    });

    Ok(CmdExeCode::Ok)
}

fn find_peer(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let pid = args.get(0).unwrap();
    let peer = PeerId::from_str(pid).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    task::block_on(async {
        let r = kad.find_peer(&peer).await;
        println!("FindPeer: {:?}", r);
    });

    Ok(CmdExeCode::Ok)
}
