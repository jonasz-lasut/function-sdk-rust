//! A spec-compliant gRPC server runtime for composition functions.

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use tonic::body::Body;
use tonic::codegen::Service;
use tonic::codegen::http;
use tonic::server::NamedService;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
pub use tonic_health::server::HealthReporter;

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

    /// Maximum size in bytes of gRPC messages the function accepts.
    /// Defaults to the gRPC default of 4MB.
    #[arg(long)]
    pub max_recv_message_size: Option<usize>,
}

/// Starts a gRPC server and serves RunFunctionRequests until SIGTERM or
/// SIGINT, then shuts down gracefully.
///
/// Serves with mTLS from `--tls-certs-dir` (tls.crt and tls.key must be the
/// function's PEM-encoded certificate and key; ca.crt a PEM-encoded CA used
/// to authenticate Crossplane) unless `--insecure` is set. gRPC server
/// reflection is enabled for both the v1 and v1alpha reflection APIs, and
/// the gRPC health service reports the function as serving.
pub async fn serve<F: FunctionRunnerService>(function: F, args: &Args) -> Result<(), Error> {
    let mut function_service = FunctionRunnerServiceServer::new(function);
    if let Some(size) = args.max_recv_message_size {
        function_service = function_service.max_decoding_message_size(size);
    }
    serve_service(function_service, args).await
}

/// Like [`serve`], but serves any gRPC service in place of the generated
/// FunctionRunnerService server, with the same spec-compliant transport:
/// mTLS from `--tls-certs-dir` unless `--insecure`, v1 and v1alpha gRPC
/// server reflection, the gRPC health service reporting the service (by its
/// [`NamedService::NAME`]) as serving, and graceful shutdown on SIGTERM or
/// SIGINT.
///
/// Use it when the generated server's prost codec is not enough - a runtime
/// that must forward request bytes verbatim (prost drops fields newer than
/// the generated types, so a transparent proxy needs its own codec), or a
/// service wrapped for instrumentation. The service owns its own message
/// decoding, so `--max-recv-message-size` is its business to enforce;
/// [`Args::max_recv_message_size`] carries what the caller asked for.
pub async fn serve_service<S>(service: S, args: &Args) -> Result<(), Error>
where
    S: Service<
            http::Request<Body>,
            Response = http::Response<Body>,
            Error = std::convert::Infallible,
        > + NamedService
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    let (health_reporter, server) = serve_service_with_health(service, args).await?;
    health_reporter.set_serving::<S>().await;
    server.await
}

/// Like [`serve_service`], but hands back the gRPC health reporter next to
/// the server future instead of reporting the service as serving up front.
///
/// The service starts as NOT SERVING; flip it with
/// [`HealthReporter::set_serving`] once the function is ready - typically
/// from a start-up task (warming caches, loading modules) running while the
/// server already answers. Requests are served regardless of health status:
/// health is what probes read, not a gate. Nothing listens until the
/// returned future is awaited.
///
/// ```no_run
/// # async fn example<S>(service: S, args: &function_sdk_rust::Args)
/// # -> Result<(), function_sdk_rust::Error>
/// # where S: tonic::codegen::Service<
/// #         tonic::codegen::http::Request<tonic::body::Body>,
/// #         Response = tonic::codegen::http::Response<tonic::body::Body>,
/// #         Error = std::convert::Infallible,
/// #     > + tonic::server::NamedService + Clone + Send + Sync + 'static,
/// #     S::Future: Send + 'static,
/// # {
/// let (health, server) = function_sdk_rust::serve_service_with_health(service, args).await?;
/// tokio::spawn(async move {
///     // ... warm caches ...
///     health.set_serving::<S>().await;
/// });
/// server.await
/// # }
/// ```
pub async fn serve_service_with_health<S>(
    service: S,
    args: &Args,
) -> Result<
    (
        HealthReporter,
        impl Future<Output = Result<(), Error>> + use<S>,
    ),
    Error,
>
where
    S: Service<
            http::Request<Body>,
            Response = http::Response<Body>,
            Error = std::convert::Infallible,
        > + NamedService
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
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

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter.set_not_serving::<S>().await;

    let insecure = args.insecure;
    let server = async move {
        tracing::info!(%address, insecure, service = S::NAME, "serving");
        builder
            .add_service(service)
            .add_service(health_service)
            .add_service(reflection_v1)
            .add_service(reflection_v1alpha)
            .serve_with_shutdown(address, shutdown_signal())
            .await?;
        Ok(())
    };
    Ok((health_reporter, server))
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
