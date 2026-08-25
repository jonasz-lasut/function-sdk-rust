use crossplane_function_sdk::proto::v1::function_runner_service_client::FunctionRunnerServiceClient;
use crossplane_function_sdk::proto::v1::function_runner_service_server::FunctionRunnerService;
use crossplane_function_sdk::proto::v1::{RequestMeta, RunFunctionRequest, RunFunctionResponse};
use crossplane_function_sdk::{Args, response, serve};
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
