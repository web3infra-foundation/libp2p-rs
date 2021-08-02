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

use crate::control::Control;
use crate::TopicHash;
use libp2prs_runtime::task;
use xcli::{App, CmdExeCode, Command, XcliError, XcliResult};

const GOSSIP: &str = "gossip";

pub fn gossip_cli_commands<'a>() -> Command<'a> {
    Command::new_with_alias(GOSSIP, "g")
        .about("Gossip")
        .usage("gossip")
        .subcommand(
            Command::new_with_alias("mesh", "m")
                .about("dump mesh peer")
                .usage("mesh")
                .action(cli_gossip_dump_mesh),
        )
        .subcommand(
            Command::new_with_alias("fanout", "fan")
                .about("dump fan-out peer")
                .usage("fanout")
                .action(cli_gossip_dump_fanout),
        )
        .subcommand(
            Command::new_with_alias("subscribe", "sub")
                .about("subscribe a specified topic")
                .usage("subscribe <topic>")
                .action(cli_gossip_subscribe),
        )
        .subcommand(
            Command::new_with_alias("unsubscribe", "unsub")
                .about("unsubscribe a specified topic")
                .usage("unsubscribe <topic>")
                .action(cli_gossip_unsubscribe),
        )
        .subcommand(
            Command::new_with_alias("publish", "pub")
                .about("publish a message to specified topic")
                .usage("publish <topic> <message>")
                .action(cli_gossip_publish),
        )
}

//
fn handler(app: &App) -> Option<Control> {
    match app.get_handler(GOSSIP) {
        Ok(value_any) => {
            let gossip = value_any.downcast_ref::<Control>().expect("control").clone();
            Some(gossip)
        }
        Err(_) => None,
    }
}

/// Show all mesh peers info
fn cli_gossip_dump_mesh(app: &App, _args: &[&str]) -> XcliResult {
    if let Some(mut gossip) = handler(app) {
        task::block_on(async {
            let stats = gossip.dump_mesh_peer().await;
            println!("Mesh list: ");
            if let Ok(peer_list) = stats {
                for peer in peer_list {
                    println!("{:?}", peer);
                }
            }
        });

        Ok(CmdExeCode::Ok)
    } else {
        Err(XcliError::MissingHandler("Gossip is not supported.".to_string()))
    }
}

/// Show all fan-out peers info
fn cli_gossip_dump_fanout(app: &App, _args: &[&str]) -> XcliResult {
    if let Some(mut gossip) = handler(app) {
        task::block_on(async {
            let stats = gossip.dump_fanout_peer().await;
            println!("Fan-out list: ");
            if let Ok(peer_list) = stats {
                for peer in peer_list {
                    println!("{:?}", peer);
                }
            }
        });

        Ok(CmdExeCode::Ok)
    } else {
        Err(XcliError::MissingHandler("Gossip is not supported.".to_string()))
    }
}

/// Subscribe to a specific topic.
fn cli_gossip_subscribe(app: &App, args: &[&str]) -> XcliResult {
    if let Some(mut gossip) = handler(app) {
        let topic = match args.len() {
            1 => TopicHash::from_raw(args[0]),
            _ => return Err(XcliError::MismatchArgument(1, args.len())),
        };
        task::block_on(async {
            let mut subscription = gossip
                .subscribe(topic.clone())
                .await
                .map_err(|e| XcliError::Other(format!("{:?}", e)))?;
            task::spawn(async move {
                while let Some(message) = subscription.next().await {
                    log::info!("Cli message info: {:?}", message);
                }

                log::info!("Topic {:?} is unsubscribed", topic);
            });

            Ok(CmdExeCode::Ok)
        })
    } else {
        Err(XcliError::MissingHandler("Gossip is not supported.".to_string()))
    }
}

/// Unsubscribe topic.
fn cli_gossip_unsubscribe(app: &App, args: &[&str]) -> XcliResult {
    if let Some(mut gossip) = handler(app) {
        let topic = match args.len() {
            1 => TopicHash::from_raw(args[0]),
            _ => return Err(XcliError::MismatchArgument(1, args.len())),
        };
        task::block_on(async {
            let _ = gossip.unsubscribe(topic.clone()).await;

            Ok(CmdExeCode::Ok)
        })
    } else {
        Err(XcliError::MissingHandler("Gossip is not supported.".to_string()))
    }
}

/// publish message to specific topic.
fn cli_gossip_publish(app: &App, args: &[&str]) -> XcliResult {
    if let Some(mut gossip) = handler(app) {
        let (topic, data) = match args.len() {
            2 => {
                let topic = TopicHash::from_raw(args[0]);
                let data = args[1];
                (topic, data)
            }
            _ => return Err(XcliError::MismatchArgument(2, args.len())),
        };

        task::block_on(async {
            let _ = gossip
                .publish(topic, data)
                .await
                .map_err(|e| XcliError::Other(format!("{:?}", e)))?;

            Ok(CmdExeCode::Ok)
        })
    } else {
        Err(XcliError::MissingHandler("Gossip is not supported.".to_string()))
    }
}

// fn cli_gossip_peer_topic(app: &App, args: &[&str]) -> XcliResult {
//     if let Some(gossip) = handler(app) {
//         let peer_id = match args.len() {
//             1 => PeerId::from_str(args[0]).map_err(|e| XcliError::Other(format!("{:?}", e)))?,
//             _ => return Err(XcliError::MismatchArgument(1, args.len())),
//         };
//
//         gossip.
//     } else {
//         return Err(XcliError::MissingHandler("Gossip is not supported.".to_string()));
//     }
//     Ok(CmdExeCode::Ok)
// }
