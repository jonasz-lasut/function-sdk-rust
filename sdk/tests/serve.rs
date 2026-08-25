use function_sdk_rust::proto::v1::function_runner_service_client::FunctionRunnerServiceClient;
use function_sdk_rust::proto::v1::function_runner_service_server::FunctionRunnerService;
use function_sdk_rust::proto::v1::function_runner_service_server::FunctionRunnerServiceServer;
use function_sdk_rust::proto::v1::{RequestMeta, RunFunctionRequest, RunFunctionResponse};
use function_sdk_rust::{Args, response, serve, serve_customized};
use tonic::{Request, Response, Status};

struct EchoFunction;

#[tonic::async_trait]
impl FunctionRunnerService for EchoFunction {
    async fn run_function(
        &self,
        request: Request<RunFunctionRequest>,
    ) -> Result<Response<RunFunctionResponse>, Status> {
        let req = request.into_inner();
        let mut rsp = response::to(&req, response::DEFAULT_TTL);
        response::normal(&mut rsp, "echo");
        Ok(Response::new(rsp))
    }
}

#[tokio::test]
async fn serve_handles_a_run_function_request() {
    let port = free_port();
    tokio::spawn(async move {
        let args = Args {
            debug: false,
            address: format!("127.0.0.1:{port}"),
            tls_certs_dir: None,
            insecure: true,
            max_recv_message_size: None,
        };
        serve(EchoFunction, &args).await.expect("serve must start");
    });

    let mut client = connect_with_retry(port).await;

    let rsp = client
        .run_function(RunFunctionRequest {
            meta: Some(RequestMeta {
                tag: "smoke".to_string(),
                capabilities: vec![],
            }),
            ..Default::default()
        })
        .await
        .expect("RunFunction must succeed")
        .into_inner();

    assert_eq!(rsp.meta.unwrap().tag, "smoke");
    assert_eq!(rsp.results[0].message, "echo");

    let channel = tonic::transport::Channel::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("valid URI")
        .connect()
        .await
        .expect("health channel must connect");
    let mut health = tonic_health::pb::health_client::HealthClient::new(channel);
    let status = health
        .check(tonic_health::pb::HealthCheckRequest {
            service: "apiextensions.fn.proto.v1.FunctionRunnerService".to_string(),
        })
        .await
        .expect("health check must succeed")
        .into_inner();
    assert_eq!(
        status.status,
        tonic_health::pb::health_check_response::ServingStatus::Serving as i32
    );
}

/// A hand-rolled tonic service - what serve_service exists for. It
/// delegates to the generated server, but as far as serve_service can tell
/// it is an arbitrary Service + NamedService implementation.
#[derive(Clone)]
struct CustomService(FunctionRunnerServiceServer<EchoFunction>);

impl tonic::server::NamedService for CustomService {
    // NAME both routes requests and keys the health service, so a service
    // that stands in for the FunctionRunnerService keeps its name.
    const NAME: &'static str =
        <FunctionRunnerServiceServer<EchoFunction> as tonic::server::NamedService>::NAME;
}

impl tonic::codegen::Service<tonic::codegen::http::Request<tonic::body::Body>> for CustomService {
    type Response = tonic::codegen::http::Response<tonic::body::Body>;
    type Error = std::convert::Infallible;
    type Future = <FunctionRunnerServiceServer<EchoFunction> as tonic::codegen::Service<
        tonic::codegen::http::Request<tonic::body::Body>,
    >>::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        <FunctionRunnerServiceServer<EchoFunction> as tonic::codegen::Service<
            tonic::codegen::http::Request<tonic::body::Body>,
        >>::poll_ready(&mut self.0, cx)
    }

    fn call(&mut self, req: tonic::codegen::http::Request<tonic::body::Body>) -> Self::Future {
        <FunctionRunnerServiceServer<EchoFunction> as tonic::codegen::Service<
            tonic::codegen::http::Request<tonic::body::Body>,
        >>::call(&mut self.0, req)
    }
}

#[tokio::test]
async fn serve_customized_serves_a_custom_service() {
    let port = free_port();
    tokio::spawn(async move {
        let args = Args {
            debug: false,
            address: format!("127.0.0.1:{port}"),
            tls_certs_dir: None,
            insecure: true,
            max_recv_message_size: None,
        };
        let service = CustomService(FunctionRunnerServiceServer::new(EchoFunction));
        serve_customized(&args)
            .service(service)
            .serve()
            .await
            .expect("serve must start");
    });

    let mut client = connect_with_retry(port).await;

    let rsp = client
        .run_function(RunFunctionRequest {
            meta: Some(RequestMeta {
                tag: "custom".to_string(),
                capabilities: vec![],
            }),
            ..Default::default()
        })
        .await
        .expect("RunFunction must succeed")
        .into_inner();

    assert_eq!(rsp.meta.unwrap().tag, "custom");
    assert_eq!(rsp.results[0].message, "echo");

    // Health is keyed by S::NAME.
    let channel = tonic::transport::Channel::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("valid URI")
        .connect()
        .await
        .expect("health channel must connect");
    let mut health = tonic_health::pb::health_client::HealthClient::new(channel);
    let status = health
        .check(tonic_health::pb::HealthCheckRequest {
            service: "apiextensions.fn.proto.v1.FunctionRunnerService".to_string(),
        })
        .await
        .expect("health check must succeed")
        .into_inner();
    assert_eq!(
        status.status,
        tonic_health::pb::health_check_response::ServingStatus::Serving as i32
    );
}

#[tokio::test]
async fn serve_customized_hands_health_to_the_caller() {
    let port = free_port();
    let args = Args {
        debug: false,
        address: format!("127.0.0.1:{port}"),
        tls_certs_dir: None,
        insecure: true,
        max_recv_message_size: None,
    };
    let mut builder = serve_customized(&args).function(EchoFunction);
    let health = builder.health_reporter();
    health
        .set_not_serving::<FunctionRunnerServiceServer<EchoFunction>>()
        .await;
    tokio::spawn(async move {
        builder.serve().await.expect("serve must start");
    });

    let mut client = connect_with_retry(port).await;

    let channel = tonic::transport::Channel::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("valid URI")
        .connect()
        .await
        .expect("health channel must connect");
    let mut health_client = tonic_health::pb::health_client::HealthClient::new(channel);
    let check =
        |c: &mut tonic_health::pb::health_client::HealthClient<tonic::transport::Channel>| {
            let mut c = c.clone();
            async move {
                c.check(tonic_health::pb::HealthCheckRequest {
                    service: "apiextensions.fn.proto.v1.FunctionRunnerService".to_string(),
                })
                .await
                .expect("health check must succeed")
                .into_inner()
                .status
            }
        };

    // Not serving until the caller says so - but requests are answered:
    // health is what probes read, not a gate.
    assert_eq!(
        check(&mut health_client).await,
        tonic_health::pb::health_check_response::ServingStatus::NotServing as i32
    );
    let rsp = client
        .run_function(RunFunctionRequest {
            meta: Some(RequestMeta {
                tag: "early".to_string(),
                capabilities: vec![],
            }),
            ..Default::default()
        })
        .await
        .expect("RunFunction must succeed while not serving")
        .into_inner();
    assert_eq!(rsp.results[0].message, "echo");

    health
        .set_serving::<FunctionRunnerServiceServer<EchoFunction>>()
        .await;
    assert_eq!(
        check(&mut health_client).await,
        tonic_health::pb::health_check_response::ServingStatus::Serving as i32
    );
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind to an ephemeral port")
        .local_addr()
        .expect("read local addr")
        .port()
}

async fn connect_with_retry(port: u16) -> FunctionRunnerServiceClient<tonic::transport::Channel> {
    for _ in 0..50 {
        if let Ok(client) =
            FunctionRunnerServiceClient::connect(format!("http://127.0.0.1:{port}")).await
        {
            return client;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("cannot connect to the function under test");
}
