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

## Differences from function-sdk-go

This SDK covers the same RunFunction protocol surface as [function-sdk-go],
with a few deliberate differences:

- **v1 only.** The v1beta1 compatibility service is not implemented.
  Functions built with this SDK require Crossplane v1.17 or later; all older
  versions are end of life.
- **No typed composite/composed resource wrappers.** function-sdk-go wraps
  resources in `composite.Unstructured`/`composed.Unstructured` with
  fieldpath accessors (`GetString("spec.region")`) and Crossplane machinery
  accessors, built on crossplane-runtime. The idiomatic Rust equivalent is
  serde: deserialize observed resources into your own structs (or use
  `serde_json::Value::pointer` for ad hoc paths) via
  `resource::struct_to_json`, and build desired state from any
  `serde::Serialize` value via `resource::update`. Integral numbers survive
  the protobuf Struct round-trip, so integer fields deserialize cleanly.
- **Health is always on.** function-sdk-go's health service is opt-in
  (`WithHealthServer`); this SDK always serves the gRPC health API and
  reports the function as serving.
- **Graceful shutdown is built in.** The server drains in-flight requests on
  SIGTERM and SIGINT.
- **No Prometheus metrics yet.** function-sdk-go serves gRPC server metrics
  on `:8080` (`WithMetricsServer`). As of August 2026 there is no maintained
  tonic-compatible Prometheus layer: the only purpose-built crate,
  [tonic-prometheus-layer], is pinned to tonic 0.13 and lags tonic majors,
  so depending on it would hold this SDK back. When metrics are needed the
  plan is to hand-roll a small tower layer on [prometheus-client] (the
  actively maintained official Rust client) with a `/metrics` endpoint,
  rather than take that dependency.

[crossplane]: https://www.crossplane.io
[functions]: https://docs.crossplane.io/latest/composition/compositions/
[function-sdk-python]: https://github.com/crossplane/function-sdk-python
[function-sdk-go]: https://github.com/crossplane/function-sdk-go
[spec]: https://github.com/crossplane/crossplane/blob/main/contributing/specifications/functions.md
[proto]: https://github.com/crossplane/crossplane/tree/main/proto/fn/v1
[tonic-prometheus-layer]: https://crates.io/crates/tonic-prometheus-layer
[prometheus-client]: https://crates.io/crates/prometheus-client
