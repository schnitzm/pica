// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::Result;
use clap::Parser;
use env_logger::Env;
use pica::{Pica, PicaCommand};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::try_join;

const DEFAULT_UCI_PORT: u16 = 7000;

async fn accept_incoming(cmd_tx: mpsc::Sender<PicaCommand>, uci_port: u16) -> Result<()> {
    let uci_socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, uci_port);
    let uci_listener = TcpListener::bind(uci_socket).await?;
    log::info!("? main");
    log::info!("Pica: Listening on: {}", uci_port);

    loop {
        let (socket, addr) = uci_listener.accept().await?;
        log::info!("Uwb host addr: {}", addr);

        let (read_half, write_half) = socket.into_split();
        let stream = Box::pin(futures::stream::unfold(read_half, pica::packets::uci::read));
        let sink = Box::pin(futures::sink::unfold(write_half, pica::packets::uci::write));

        cmd_tx
            .send(PicaCommand::Connect(stream, sink))
            .await
            .map_err(|_| anyhow::anyhow!("pica command stream closed"))?
    }
}

#[derive(Parser, Debug)]
#[command(name = "pica", about = "Virtual UWB subsystem")]
struct Args {
    /// Output directory for storing .pcapng traces.
    /// If provided, .pcapng traces of client connections are automatically
    /// saved under the name `device-{handle}.pcapng`.
    #[arg(short, long, value_name = "PCAPNG_DIR")]
    pcapng_dir: Option<PathBuf>,
    /// Configure the TCP port for the UCI server.
    #[arg(short, long, value_name = "UCI_PORT", default_value_t = DEFAULT_UCI_PORT)]
    uci_port: u16,
}

struct MockRangingEstimator();

/// The position cannot be communicated to the pica environment when
/// using the default binary (HTTP interface not available).
/// Thus the ranging estimator cannot produce any result.
impl pica::RangingEstimator for MockRangingEstimator {
    fn estimate(
        &self,
        _left: &pica::Handle,
        _right: &pica::Handle,
    ) -> Option<pica::RangingMeasurement> {
        Some(Default::default())
    }
}

#[tokio::main]
async fn main() -> Result<()> {

    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();
    log::info!("bin main");
    let args = Args::parse();

    let pica = Pica::new(Box::new(MockRangingEstimator()), args.pcapng_dir);
    let commands = pica.commands();

    try_join!(accept_incoming(commands.clone(), args.uci_port), pica.run(),)?;

    Ok(())
}
