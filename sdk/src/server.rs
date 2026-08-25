//! A spec-compliant gRPC server runtime for composition functions.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use tonic::body::Body;
use tonic::codegen::Service;
use tonic::codegen::http;
use tonic::server::NamedService;
use tonic::service::Routes;
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
///
/// This is the default server; [`serve_customized`] builds one from parts.
pub async fn serve<F: FunctionRunnerService>(function: F, args: &Args) -> Result<(), Error> {
    serve_customized(args).function(function).serve().await
}

/// Starts building a customized function server: the same spec-compliant
/// transport as [`serve`] (mTLS from `--tls-certs-dir` unless `--insecure`,
/// v1 and v1alpha gRPC server reflection, the gRPC health service, graceful
/// shutdown on SIGTERM or SIGINT), composed from the components you add.
///
/// Add the function itself with [`ServerBuilder::function`] (the typed
/// service [`serve`] runs) or [`ServerBuilder::service`] (any tonic
/// service - a custom codec, an instrumented wrapper), further gRPC
/// services with more [`ServerBuilder::service`] calls, and reflection
/// descriptors for them with [`ServerBuilder::file_descriptor_set`]. Take
/// the health reporter with [`ServerBuilder::health_reporter`] to own
/// readiness - for example, to flip to serving only after start-up work.
/// Finish with [`ServerBuilder::serve`].
///
/// ```no_run
/// # use function_sdk_rust::proto::v1::function_runner_service_server::{
/// #     FunctionRunnerService, FunctionRunnerServiceServer,
/// # };
/// # async fn example<F: FunctionRunnerService>(
/// #     function: F,
/// #     args: &function_sdk_rust::Args,
/// # ) -> Result<(), function_sdk_rust::Error> {
/// let mut builder = function_sdk_rust::serve_customized(args)
///     .function(function);
/// let health = builder.health_reporter();
/// health.set_not_serving::<FunctionRunnerServiceServer<F>>().await;
/// tokio::spawn({
///     let health = health.clone();
///     async move {
///         // ... warm caches ...
///         health.set_serving::<FunctionRunnerServiceServer<F>>().await;
///     }
/// });
/// builder.serve().await
/// # }
/// ```
pub fn serve_customized(args: &Args) -> ServerBuilder {
    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    ServerBuilder {
        address: args.address.clone(),
        insecure: args.insecure,
        tls_certs_dir: args.tls_certs_dir.clone(),
        max_recv_message_size: args.max_recv_message_size,
        routes: Routes::default().add_service(health_service),
        service_names: Vec::new(),
        descriptor_sets: vec![crate::proto::FILE_DESCRIPTOR_SET],
        health_reporter,
        health_managed: true,
    }
}

/// A function server under construction; see [`serve_customized`].
pub struct ServerBuilder {
    address: String,
    insecure: bool,
    tls_certs_dir: Option<PathBuf>,
    max_recv_message_size: Option<usize>,
    routes: Routes,
    service_names: Vec<&'static str>,
    descriptor_sets: Vec<&'static [u8]>,
    health_reporter: HealthReporter,
    health_managed: bool,
}

impl ServerBuilder {
    /// Adds the function as the generated typed FunctionRunnerService
    /// server, honoring `--max-recv-message-size` - the service [`serve`]
    /// runs.
    pub fn function<F: FunctionRunnerService>(self, function: F) -> Self {
        let mut service = FunctionRunnerServiceServer::new(function);
        if let Some(size) = self.max_recv_message_size {
            service = service.max_decoding_message_size(size);
        }
        self.service(service)
    }

    /// Adds any gRPC service. Use it for the function itself when the
    /// generated server's prost codec is not enough (prost drops fields
    /// newer than the generated types, so a transparent proxy needs its own
    /// codec - which then owns its message-size limits;
    /// [`Args::max_recv_message_size`] carries what the caller asked for),
    /// or for further services beside the function. Each added service is
    /// registered with the health service under its
    /// [`NamedService::NAME`].
    pub fn service<S>(mut self, service: S) -> Self
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
        self.routes = self.routes.add_service(service);
        self.service_names.push(S::NAME);
        self
    }

    /// Registers another encoded file descriptor set with gRPC server
    /// reflection, next to this crate's own - for services added beside the
    /// function.
    pub fn file_descriptor_set(mut self, encoded: &'static [u8]) -> Self {
        self.descriptor_sets.push(encoded);
        self
    }

    /// Hands out the gRPC health reporter and makes health the caller's:
    /// [`serve`](ServerBuilder::serve) then reports nothing by itself, so
    /// set each service's initial status (typically
    /// [`HealthReporter::set_not_serving`] before start-up work, flipped
    /// with [`HealthReporter::set_serving`] when ready). Without this call,
    /// every added service is reported as serving when serving starts.
    pub fn health_reporter(&mut self) -> HealthReporter {
        self.health_managed = false;
        self.health_reporter.clone()
    }

    /// Starts the server and serves until SIGTERM or SIGINT, then shuts
    /// down gracefully. Nothing listens before this.
    pub async fn serve(self) -> Result<(), Error> {
        let address: SocketAddr = self.address.parse()?;

        let mut builder = Server::builder();
        if !self.insecure {
            let dir = self
                .tls_certs_dir
                .as_deref()
                .ok_or(Error::MissingTlsCertsDir)?;
            builder = builder.tls_config(tls_config(dir)?)?;
        }

        let reflection = || {
            let mut builder = tonic_reflection::server::Builder::configure();
            for encoded in &self.descriptor_sets {
                builder = builder.register_encoded_file_descriptor_set(encoded);
            }
            builder
        };
        let reflection_v1 = reflection().build_v1()?;
        let reflection_v1alpha = reflection().build_v1alpha()?;

        if self.health_managed {
            for name in &self.service_names {
                self.health_reporter
                    .set_service_status(name, tonic_health::ServingStatus::Serving)
                    .await;
            }
        }

        tracing::info!(%address, insecure = self.insecure, services = ?self.service_names, "serving");

        builder
            .add_routes(self.routes)
            .add_service(reflection_v1)
            .add_service(reflection_v1alpha)
            .serve_with_shutdown(address, shutdown_signal())
            .await?;

        Ok(())
    }
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
