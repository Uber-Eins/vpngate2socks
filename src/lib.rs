//! Linux-first, leak-resistant VPN Gate to local SOCKS5 service.

pub mod api;
mod auto_connect;
mod automatic_tests;
pub mod config;
pub mod domain;
mod mihomo;
pub mod netd;
pub mod openvpn;
pub mod quality;
pub mod service;
pub mod socks;
pub mod storage;
mod test_registry;
pub mod vpngate;

use std::{path::PathBuf, time::Duration};

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::config::AppConfig;

/// Command-line interface for the control plane, helper, and worker roles.
#[derive(Debug, Parser)]
#[command(name = "vpngate2socks", version, about)]
pub struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Runs the unprivileged Web/API and local SOCKS control plane.
    Serve,
    /// Runs the privileged network namespace and `OpenVPN` helper.
    Netd,
    /// Runs a private SOCKS server inside one worker network namespace.
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        socket: PathBuf,
        #[arg(long)]
        require_tun: bool,
    },
    /// Applies per-network-namespace kernel hardening from a private mount namespace.
    #[command(hide = true)]
    NamespaceSetup,
}

/// Installs the process-wide structured tracing subscriber.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("vpngate2socks=info,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Runs the selected process role.
pub async fn run(arguments: Arguments) -> anyhow::Result<()> {
    match arguments.command.unwrap_or(Command::Serve) {
        Command::Serve => run_control_plane(AppConfig::from_env()?).await,
        Command::Netd => {
            let config = AppConfig::from_env()?;
            let shutdown = CancellationToken::new();
            spawn_signal_handler(shutdown.clone());
            netd::run_netd(config, shutdown).await?;
            Ok(())
        }
        Command::Worker {
            socket,
            require_tun,
        } => {
            let shutdown = CancellationToken::new();
            spawn_signal_handler(shutdown.clone());
            socks::run_worker(&socket, require_tun, shutdown).await?;
            Ok(())
        }
        Command::NamespaceSetup => {
            netd::configure_namespace().await?;
            Ok(())
        }
    }
}

async fn run_control_plane(config: AppConfig) -> anyhow::Result<()> {
    let shutdown = CancellationToken::new();
    spawn_signal_handler(shutdown.clone());
    if config.lan_mode && config.tls.is_none() {
        tracing::warn!("LAN 模式未配置 TLS；WebUI 与 SOCKS 凭据会以明文在局域网传输");
    }
    let netd = netd::NetdClient::new(
        config.netd_socket.clone(),
        config.connect_timeout + Duration::from_secs(10),
    );
    let upstream_address = netd
        .ping()
        .await
        .context("failed to obtain the upstream address pinned by netd")?;
    let upstream = config
        .upstream
        .resolve_to(upstream_address)
        .context("netd resolved an inconsistent upstream address")?;
    let store = storage::Store::open(&config.database_url)
        .await
        .context("failed to open application database")?;
    let auto_connect_config = store
        .auto_connect_config()
        .await
        .context("failed to load automatic connection configuration")?;
    let state = service::AppState::new(
        config.clone(),
        upstream,
        store,
        auto_connect_config,
        shutdown.clone(),
    );
    state.start_refresh_loop();

    tracing::info!(
        address = %config.web_bind,
        tls = config.tls.is_some(),
        "WebUI listener starting"
    );
    tracing::info!(address = %config.socks_bind, "SOCKS5 listener starting");
    let web_shutdown = shutdown.clone();
    let web_state = state.clone();
    let web_address = config.web_bind;
    let tls = config.tls.clone();
    let web = async move {
        let handle = axum_server::Handle::new();
        let shutdown_handle = handle.clone();
        tokio::spawn(async move {
            web_shutdown.cancelled().await;
            shutdown_handle.graceful_shutdown(Some(Duration::from_secs(10)));
        });
        let router = api::router(web_state).into_make_service();
        if let Some(tls) = tls {
            let tls = axum_server::tls_openssl::OpenSSLConfig::from_pem_file(
                tls.certificate,
                tls.private_key,
            )
            .context("failed to load TLS certificate or key")?;
            axum_server::bind_openssl(web_address, tls)
                .handle(handle)
                .serve(router)
                .await
                .context("HTTPS server failed")
        } else {
            axum_server::bind(web_address)
                .handle(handle)
                .serve(router)
                .await
                .context("HTTP server failed")
        }
    };
    let socks = socks::run_gateway(
        config.socks_bind,
        state.active_worker(),
        config.socks_credentials.clone(),
        shutdown.clone(),
    );

    tokio::pin!(web);
    tokio::pin!(socks);
    tokio::select! {
        result = &mut web => result.context("WebUI server failed")?,
        result = &mut socks => result.context("SOCKS5 server failed")?,
        () = shutdown.cancelled() => {}
    }
    shutdown.cancel();
    Ok(())
}

fn spawn_signal_handler(shutdown: CancellationToken) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
            match terminate {
                Ok(mut terminate) => {
                    tokio::select! {
                        result = tokio::signal::ctrl_c() => {
                            if let Err(error) = result {
                                tracing::error!(error = %error, "failed to listen for Ctrl-C");
                            }
                        }
                        _ = terminate.recv() => {}
                    }
                }
                Err(error) => {
                    tracing::error!(error = %error, "failed to listen for SIGTERM");
                    let _result = tokio::signal::ctrl_c().await;
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _result = tokio::signal::ctrl_c().await;
        }
        tracing::info!("shutdown signal received");
        shutdown.cancel();
    });
}
