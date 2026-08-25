//! A spec-compliant gRPC server runtime for composition functions.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};

use crate::Error;
use crate::proto::v1::function_runner_service_server::{
    FunctionRunnerService, FunctionRunnerServiceServer,
};

/// CLI arguments required by the Crossplane composition function spec.
#[derive(clap::Parser, Debug)]
#[command(version, about = "A Crossplane composition function")]
pub struct Args {
    /// Emit debug logs.
    #[arg(short, long, env = "DEBUG", default_value_t = false)]
    pub debug: bool,

    /// Address at which to listen for gRPC connections.
    #[arg(long, default_value = "0.0.0.0:9443")]
    pub address: String,

    /// Directory containing tls.crt, tls.key, and ca.crt; serve using mTLS.
    #[arg(long, env = "TLS_SERVER_CERTS_DIR")]
    pub tls_certs_dir: Option<PathBuf>,

    /// Run without mTLS credentials. If set, --tls-certs-dir is ignored.
    #[arg(long, default_value_t = false)]
    pub insecure: bool,
}

/// Starts a gRPC server and serves RunFunctionRequests until SIGTERM or
/// SIGINT, then shuts down gracefully.
///
/// Serves with mTLS from `--tls-certs-dir` (tls.crt and tls.key must be the
/// function's PEM-encoded certificate and key; ca.crt a PEM-encoded CA used
/// to authenticate Crossplane) unless `--insecure` is set. gRPC server
/// reflection is enabled for both the v1 and v1alpha reflection APIs.
pub async fn serve<F: FunctionRunnerService>(function: F, args: &Args) -> Result<(), Error> {
    let address: SocketAddr = args.address.parse()?;

    let mut builder = Server::builder();
    if !args.insecure {
        let dir = args
            .tls_certs_dir
            .as_deref()
            .ok_or(Error::MissingTlsCertsDir)?;
        builder = builder.tls_config(tls_config(dir)?)?;
    }

    let reflection_v1 = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(crate::proto::FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let reflection_v1alpha = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(crate::proto::FILE_DESCRIPTOR_SET)
        .build_v1alpha()?;

    tracing::info!(%address, insecure = args.insecure, "serving FunctionRunnerService");

    builder
        .add_service(FunctionRunnerServiceServer::new(function))
        .add_service(reflection_v1)
        .add_service(reflection_v1alpha)
        .serve_with_shutdown(address, shutdown_signal())
        .await?;

    Ok(())
}

fn tls_config(dir: &Path) -> Result<ServerTlsConfig, Error> {
    let read = |name: &str| {
        let path = dir.join(name);
        std::fs::read(&path).map_err(|source| Error::ReadCertificate { path, source })
    };
    let cert = read("tls.crt")?;
    let key = read("tls.key")?;
    let ca = read("ca.crt")?;

    Ok(ServerTlsConfig::new()
        .identity(Identity::from_pem(cert, key))
        .client_ca_root(Certificate::from_pem(ca))
        .client_auth_optional(false))
}

#[cfg(unix)]
async fn shutdown_signal() {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("cannot install SIGTERM handler");
    tokio::select! {
        _ = sigterm.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    tracing::info!("shutting down");
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
