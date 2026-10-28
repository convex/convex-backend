#![feature(coroutines)]
#![feature(iterator_try_collect)]

mod connector;
mod conversions;
mod convert;
mod convex_api;
mod log;
mod schema;
mod sync;

use std::net::{
    IpAddr,
    Ipv4Addr,
    SocketAddr,
};

use clap::Parser;
use connector::ConvexConnector;
use fivetran_common::fivetran_sdk::source_connector_server::SourceConnectorServer;
use tonic::{
    codec::CompressionEncoding,
    transport::Server,
};

/// The command-line arguments received by the connector.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The port the connector receives gRPC requests from
    #[arg(long, default_value_t = 50051)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), args.port);

    let connector = ConvexConnector;

    log::log(&format!("Starting the connector on {addr}"));
    Server::builder()
        .add_service(
            SourceConnectorServer::new(connector)
                .accept_compressed(CompressionEncoding::Gzip)
                .send_compressed(CompressionEncoding::Gzip),
        )
        .serve(addr)
        .await?;

    Ok(())
}
