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

    let find_peer_cmd = Command::new_with_alias("findnode", "fn")
        .about("find node via dht")
        .usage("findnode <peerid>")
        .action(cli_find_peer);
    let get_value_cmd = Command::new_with_alias("getvalue", "gv")
        .about("get value via dht")
        .usage("getvalue <key>")
        .action(cli_get_value);
    let put_value_cmd = Command::new_with_alias("putvalue", "pv")
        .about("put value to dht")
        .usage("putvalue <key> <value>")
        .action(cli_put_value);

    let find_providers_cmd = Command::new_with_alias("findprov", "fp")
        .about("find providers via dht")
        .usage("findprov <key>")
        .action(cli_find_providers);
    let providing_cmd = Command::new_with_alias("provide", "pr")
        .about("providing key to dht")
        .usage("provide <key>")
        .action(cli_providing);

    let dump_providers_cmd = Command::new_with_alias("local-store", "ls")
        .about("dump local-store")
        .usage("local-store")
        .action(cli_dump_storage);
    let dump_kbucket_cmd = Command::new_with_alias("k-bucket", "kb")
        .about("dump k-buckets")
        .usage("k-bucket")
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
        .subcommand(dump_providers_cmd)
        .subcommand(dump_kbucket_cmd)
        .subcommand(dump_messenger_cmd)
        .subcommand(dump_stats_cmd)
        .subcommand(find_peer_cmd)
        .subcommand(get_value_cmd)
        .subcommand(put_value_cmd)
        .subcommand(find_providers_cmd)
        .subcommand(providing_cmd)
}

fn handler(app: &App) -> Control {
    let value_any = app.get_handler(DHT).expect(DHT);
    let kad = value_any.downcast_ref::<Control>().expect("control").clone();
    kad
}

fn cli_close_kad(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);
    kad.close();
    Ok(CmdExeCode::Ok)
}

fn cli_bootstrap(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);
    task::block_on(async {
        println!("start bootstrapping...");
        let _ = kad.bootstrap(vec![]).await;
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

fn cli_dump_storage(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    let _verbose = !args.is_empty();

    task::block_on(async {
        let storage_stat = kad.dump_storage().await.unwrap();
        println!("Providers: ");
        for provider in storage_stat.provider {
            println!("{} {:?} {:?}", provider.provider, provider.time_received, provider.key);
        }
        // println!("Key Value Publisher Expire");
        println!("Records: ");
        for record in storage_stat.record {
            println!(
                "{:?}, {:?}, {:?}, {:?}",
                record.key, record.value, record.time_received, record.publisher
            );
        }
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
        println!("Total refreshes        : {}", stats.total_refreshes);
        println!("Iter query executed    : {}", stats.query.iter_query_executed);
        println!("Iter query completed   : {}", stats.query.iter_query_completed);
        println!("Iter query timeout     : {}", stats.query.iter_query_timeout);
        println!("Fixed query executed   : {}", stats.query.fixed_query_executed);
        println!("Fixed query completed  : {}", stats.query.fixed_query_completed);
        let running = stats.query.iter_query_executed - stats.query.iter_query_completed - stats.query.iter_query_timeout;
        println!("Iter query in progress : {}", running);
        let running = stats.query.fixed_query_executed - stats.query.fixed_query_completed;
        println!("Fixed query in progress: {}", running);
        println!("Iter query details     : {:?}", stats.query.iterative);
        println!("Fixed query tx error   : {}", stats.query.fixed_tx_error);
        println!("Kad tx messages        : {:?}", stats.query.message_tx);
        println!("Kad rx messages        : {:?}", stats.message_rx);
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

fn cli_get_value(app: &App, args: &[&str]) -> XcliResult {
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

fn cli_put_value(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 2 {
        return Err(XcliError::MismatchArgument(2, args.len()));
    }

    let key = args.get(0).cloned().unwrap();
    let value = args.get(1).cloned().unwrap();

    task::block_on(async {
        let r = kad.put_value(Vec::from(key), Vec::from(value)).await;
        println!("put value: {:?}", r.is_ok());
    });

    Ok(CmdExeCode::Ok)
}

fn cli_find_peer(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let pid = args.get(0).unwrap();
    let peer = PeerId::from_str(pid).map_err(|e| XcliError::BadArgument(e.to_string()))?;

    task::block_on(async {
        let r = kad.find_peer(&peer).await;
        println!("Find Node: {:?}", r);
    });

    Ok(CmdExeCode::Ok)
}

fn cli_find_providers(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let key = args.get(0).unwrap();

    task::block_on(async {
        let r = kad.find_providers(key.as_bytes().into(), 0).await;
        println!("Find Providers: {:?}", r);
    });

    Ok(CmdExeCode::Ok)
}

fn cli_providing(app: &App, args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    if args.len() != 1 {
        return Err(XcliError::MismatchArgument(1, args.len()));
    }

    let key = args.get(0).unwrap();

    task::block_on(async {
        let r = kad.provide(key.as_bytes().into()).await;
        println!("Providing: {:?}", r);
    });

    Ok(CmdExeCode::Ok)
}
