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

use libp2prs_core::PeerId;
use libp2prs_runtime::task;
use libp2prs_swarm::Control;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tide::http::mime;
use tide::{Body, Request, Response, Server};

#[macro_use]
extern crate lazy_static;

lazy_static! {
    static ref NON_PARAM_ROUTE: Vec<String> = {
        vec![
            "".to_string(),
            "/recv".to_string(),
            "/send".to_string(),
            "/peer".to_string(),
            "/connection".to_string(),
        ]
    };
    static ref PARAM_ROUTE: Vec<String> = vec!["/peer/_".to_string(), "/protocol?protocol_id=_".to_string()];
}

/// Response, message contains error info if statusCode isn't 200.
#[derive(Serialize, Deserialize)]
struct ResponseBody {
    status: i64,
    message: String,
    result: Vec<String>,
}

/// Tide server
pub struct InfoServer {
    monitor: Server<Control>,
    // map: HashMap<String, IRouteHandler>,
}

/// Save package count&size
#[derive(Serialize, Deserialize)]
struct PackageInfo {
    package_count: usize,
    package_bytes: usize,
}

/// Save package count&size by peer_id or protocol_id
#[derive(Serialize, Deserialize)]
struct SpecInfo {
    package_in: usize,
    package_out: usize,
}

/// A struct that deserialize protocol_id.
#[derive(Serialize, Deserialize, Debug)]
struct Protocol {
    protocol_id: String,
}

/// A struct that deserialize peer_id.
#[derive(Serialize, Deserialize, Debug)]
struct Peer {
    count: usize,
}

/// Save data from network_info.
#[derive(Serialize, Deserialize, Debug)]
struct NetworkConnectionStatus {
    /// The total number of connections, both established and pending.
    num_connections: usize,
    /// The total number of pending connections, both incoming and outgoing.
    num_connections_pending: usize,
    /// The total number of established connections.
    num_connections_established: usize,
    /// The total number of active sub streams.
    num_active_streams: usize,
    /// The information of all established connections.
    connection_info: Vec<NetworkConnectionInfo>,
}

/// A struct that save connection info.
#[derive(Serialize, Deserialize, Debug)]
struct NetworkConnectionInfo {
    la: Vec<u8>,
    ra: Vec<u8>,
    local_peer_id: String,
    remote_peer_id: String,
    num_inbound_streams: usize,
    num_outbound_streams: usize,
}

impl InfoServer {
    pub fn new(control: Control) -> Self {
        let mut monitor = tide::with_state(control);

        monitor.at("").get(get_all);
        monitor.at("/recv").get(get_recv_pkg);
        monitor.at("/send").get(get_sent_pkg);
        monitor.at("/protocol").get(get_protocol_info);
        monitor.at("/peer").get(get_peer_count).at("/:peer_id").get(get_peer_info);
        monitor.at("/connection").get(get_connection_info);

        InfoServer { monitor }
    }

    pub fn start(self, addr: String) {
        task::spawn(async move {
            let r = self.monitor.listen(addr).await;
            log::info!("Info server started result={:?}", r);
        });
    }
}

/// Return route list
async fn get_all(req: Request<Control>) -> tide::Result {
    let addr = req.local_addr().unwrap();

    let mut available = "<h3>Available Endpoints:</h3></br>".to_string();
    for item in NON_PARAM_ROUTE.iter() {
        let route = addr.to_owned() + item;
        available = available + &format!("<a href=//{}>{}</a></br>", route, route);
    }

    let mut argument = "<h3>Endpoints that require arguments:</h3></br>".to_string();
    for item in PARAM_ROUTE.iter() {
        let route = addr.to_owned() + item;
        argument += &format!("<a href=//{}>{}</a></br>", route, route);
    }

    let res_body =
        "<head><link rel=\"icon\" href=\"data:;base64,=\"></head>".to_string() + "<body>" + &available + &argument + "</body>";

    let response = Response::builder(200).content_type(mime::HTML).body(res_body).build();
    Ok(response)
}

/// Get peer count
async fn get_peer_count(req: Request<Control>) -> tide::Result {
    let mut control = req.state().clone();

    let network_info = control.retrieve_networkinfo().await.map_err(|e| {
        log::error!("{:?}", e);
        tide::Error::new(500, e)
    })?;

    let peer = serde_json::to_string(&Peer {
        count: network_info.num_peers,
    })
    .unwrap();

    let result_body = Body::from_json(&ResponseBody {
        status: 0,
        message: "".to_string(),
        result: vec![peer],
    })?;
    let response = Response::builder(200).body(result_body).build();
    Ok(response)
}

/// Get connection info
async fn get_connection_info(req: Request<Control>) -> tide::Result {
    let mut control = req.state().clone();

    let network_info = control.retrieve_networkinfo().await.map_err(|e| {
        log::error!("{:?}", e);
        tide::Error::new(500, e)
    })?;
    let cis = control.dump_connections(None).await.map_err(|e| {
        log::error!("{:?}", e);
        tide::Error::new(500, e)
    })?;

    let mut connection_info = Vec::new();
    for item in cis {
        let info = NetworkConnectionInfo {
            la: item.info.la.to_vec(),
            ra: item.info.ra.to_vec(),
            local_peer_id: item.info.local_peer_id.to_string(),
            remote_peer_id: item.info.remote_peer_id.to_string(),
            num_inbound_streams: item.info.num_inbound_streams,
            num_outbound_streams: item.info.num_outbound_streams,
        };
        connection_info.push(info);
    }

    let network_connection_status = NetworkConnectionStatus {
        num_connections: network_info.num_connections,
        num_connections_pending: network_info.num_connections_pending,
        num_connections_established: network_info.num_connections_established,
        num_active_streams: network_info.num_active_streams,
        connection_info,
    };

    let result_body = Body::from_json(&ResponseBody {
        status: 0,
        message: "".to_string(),
        result: vec![serde_json::to_string(&network_connection_status).unwrap()],
    })?;
    let response = Response::builder(200).body(result_body).build();
    Ok(response)
}

/// Get received package counts and bytes
async fn get_recv_pkg(req: Request<Control>) -> tide::Result {
    let (package_count, package_bytes) = req.state().get_recv_count_and_size();

    let package = PackageInfo {
        package_count,
        package_bytes,
    };

    let result_body = Body::from_json(&package)?;
    let response = Response::builder(200).body(result_body).build();
    Ok(response)
}

/// Get sent package counts and bytes
async fn get_sent_pkg(req: Request<Control>) -> tide::Result {
    let (package_count, package_bytes) = req.state().get_sent_count_and_size();

    let package = PackageInfo {
        package_count,
        package_bytes,
    };

    let result_body = Body::from_json(&ResponseBody {
        status: 0,
        message: "".to_string(),
        result: vec![serde_json::to_string(&package).unwrap()],
    })?;
    let response = Response::builder(200).body(result_body).build();
    Ok(response)
}

/// Get sent&received package bytes by protocol_id
async fn get_protocol_info(req: Request<Control>) -> tide::Result {
    let protocol: Protocol = req.query()?;
    let (receive, send) = req.state().get_protocol_in_and_out(&protocol.protocol_id);

    let mut spec_info = SpecInfo {
        package_in: 0,
        package_out: 0,
    };
    if let Some(value) = receive {
        spec_info.package_in = value
    }
    if let Some(value) = send {
        spec_info.package_out = value
    }

    let result_body = Body::from_json(&ResponseBody {
        status: 0,
        message: "".to_string(),
        result: vec![serde_json::to_string(&spec_info).unwrap()],
    })?;
    let response = Response::builder(200).body(result_body).build();
    Ok(response)
}

/// Get sent&received package bytes by peer_id
async fn get_peer_info(req: Request<Control>) -> tide::Result {
    let peer = req.param("peer_id")?;
    let peer_id = match PeerId::from_str(peer) {
        Ok(info) => info,
        Err(e) => {
            let err_body = Body::from_json(&ResponseBody {
                status: 1,
                message: format!("Cannot parse : {:?}", e),
                result: vec![],
            })?;
            return Ok(Response::builder(400).body(err_body).build());
        }
    };

    let (receive, send) = req.state().get_peer_in_and_out(&peer_id);

    let mut spec_info = SpecInfo {
        package_in: 0,
        package_out: 0,
    };
    if let Some(value) = receive {
        spec_info.package_in = value
    }
    if let Some(value) = send {
        spec_info.package_out = value
    }

    let result_body = Body::from_json(&ResponseBody {
        status: 0,
        message: "".to_string(),
        result: vec![serde_json::to_string(&spec_info).unwrap()],
    })?;
    let response = Response::builder(200).body(result_body).build();
    Ok(response)
}
