use anyhow::Context as _;
use clap::Parser as _;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    vpngate2socks::init_tracing();
    let arguments = vpngate2socks::Arguments::parse();
    vpngate2socks::run(arguments)
        .await
        .context("vpngate2socks terminated with an error")
}
