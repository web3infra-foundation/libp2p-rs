use async_std::task;
use std::str::FromStr;

use crate::Control;
use libp2prs_core::{Multiaddr, PeerId};
use xcli::*;

const DHT: &str = "dht";

pub fn dht_cli_commands<'a>() -> Command<'a> {
    let bootstrap_cmd = Command::new("bootstrap")
        .about("Show or edit the list of bootstrap peers")
        .usage("bootstrap")
        .action(bootstrap);
    let add_node_cmd = Command::new("add")
        .about("Add peer to KBucket")
        .usage("add [<peer>] [<multi_address>]")
        .action(add_node);
    let rm_node_cmd = Command::new("rm")
        .about("Remove peer from KBucket")
        .usage("rm [<peer>] [<multi_address>]")
        .action(rm_node);

    let find_peer_cmd = Command::new("findpeer")
        .about("find peer through dht")
        .usage("findpeer <peerid>")
        .action(find_peer);
    let get_value_cmd = Command::new("getvalue")
        .about("get value through dht")
        .usage("getvalue <key>")
        .action(get_value);

    let dump_messenger_cmd = Command::new("dump").about("Dump KBuckets").usage("dump").action(cli_dump_kbuckets);
    let dump_dht_cmd = Command::new("messenger")
        .about("Dump Messengers")
        .usage("messengers")
        .action(cli_dump_messengers);

    Command::new("dht")
        .about("find peer or record through dht")
        .usage("dht")
        .subcommand(bootstrap_cmd)
        .subcommand(add_node_cmd)
        .subcommand(rm_node_cmd)
        .subcommand(dump_dht_cmd)
        .subcommand(dump_messenger_cmd)
        .subcommand(find_peer_cmd)
        .subcommand(get_value_cmd)
}

fn handler(app: &App) -> Control {
    let value_any = app.get_handler(DHT).expect(DHT);
    let kad = value_any.downcast_ref::<Control>().expect("control").clone();
    kad
}

fn bootstrap(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);
    task::block_on(async {
        kad.bootstrap().await;
        println!("start to bootstrap");
    });

    Ok(CmdExeCode::Ok)
}

fn add_node(app: &App, args: &[&str]) -> XcliResult {
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

fn rm_node(app: &App, args: &[&str]) -> XcliResult {
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
        let buckets = kad.dump_kbuckets().await;
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

fn cli_dump_messengers(app: &App, _args: &[&str]) -> XcliResult {
    let mut kad = handler(app);

    task::block_on(async {
        let messengers = kad.dump_messengers().await;
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
