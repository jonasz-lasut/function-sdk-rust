# function-sdk-rust

A Rust SDK for writing [Crossplane][crossplane] [composition functions][functions].

Modeled on [function-sdk-python] and [function-sdk-go]. A composition function
is a gRPC server implementing the `FunctionRunnerService` defined by
Crossplane's `apiextensions.fn.proto.v1` API; this SDK provides the generated
protocol types, a [function spec][spec] compliant server runtime, and helpers
for working with requests and responses.

## Layout

- `sdk/` - the `crossplane-function-sdk` crate.
  - `proto/v1/run_function.proto` - the vendored protocol definition.
  - `src/generated/` - checked-in code generated from the proto: prost types,
    the tonic gRPC server and client, protojson serde impls (pbjson), and the
    encoded file descriptor set used for gRPC server reflection.
  - `src/{server,request,response,resource,logging}.rs` - the hand-written
    SDK: runtime and helpers.
- `codegen/` - maintainer tool that regenerates `sdk/src/generated`.
- `example/` - an example function, the starting point for new functions.

## Writing a function

```rust
use crossplane_function_sdk::proto::v1::function_runner_service_server::FunctionRunnerService;
use crossplane_function_sdk::proto::v1::{RunFunctionRequest, RunFunctionResponse};
use crossplane_function_sdk::{resource, response};
use tonic::{Request, Response, Status};

struct Function;

#[tonic::async_trait]
impl FunctionRunnerService for Function {
    async fn run_function(
        &self,
        request: Request<RunFunctionRequest>,
    ) -> Result<Response<RunFunctionResponse>, Status> {
        let req = request.into_inner();

        // Copies the request's tag, desired state, and context forward.
        let mut rsp = response::to(&req, response::DEFAULT_TTL);

        let desired = rsp.desired.get_or_insert_default();
        let bucket = desired.resources.entry("bucket".to_string()).or_default();
        resource::update(bucket, &serde_json::json!({
            "apiVersion": "s3.aws.upbound.io/v1beta2",
            "kind": "Bucket",
            "spec": {"forProvider": {"region": "eu-central-1"}},
        })).map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(rsp))
    }
}
```

See `example/` for a complete function with a CLI entrypoint.

## Development

```shell
# Run tests.
cargo test

# Lint.
cargo clippy --workspace --tests

# Regenerate sdk/src/generated from the vendored proto (requires protoc).
cargo run -p codegen
```

The vendored proto's canonical source is
[crossplane/crossplane/proto/fn/v1][proto]. Only the v1 API is supported,
which requires Crossplane v1.17 or later.

[crossplane]: https://www.crossplane.io
[functions]: https://docs.crossplane.io/latest/composition/compositions/
[function-sdk-python]: https://github.com/crossplane/function-sdk-python
[function-sdk-go]: https://github.com/crossplane/function-sdk-go
[spec]: https://github.com/crossplane/crossplane/blob/main/contributing/specifications/functions.md
[proto]: https://github.com/crossplane/crossplane/tree/main/proto/fn/v1
