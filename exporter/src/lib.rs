pub(crate) mod exporter;

use crate::exporter::Exporter;
use async_std::task;
use libp2prs_swarm::Control;
use prometheus::{register, Encoder, TextEncoder};
use tide::{Body, Request, Response, Server};

/// Exporter server
pub struct ExporterServer {
    s: Server<()>,
}

impl ExporterServer {
    pub fn new(control: Control) -> Self {
        let mut s = tide::new();

        // Register exporter to global registry, and then we can use default gather method.
        let exporter = Exporter::new(control);
        let _ = register(Box::new(exporter));
        s.at("/metrics").get(get_metric);
        ExporterServer { s }
    }

    pub fn start(self, addr: String) {
        task::spawn(async move { self.s.listen(addr).await });
    }
}

/// Return metrics to prometheus
async fn get_metric(_: Request<()>) -> tide::Result {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];

    encoder.encode(&metric_families, &mut buffer).unwrap();

    let response = Response::builder(200)
        .content_type("text/plain; version=0.0.4")
        .body(Body::from(buffer))
        .build();

    Ok(response)
}
