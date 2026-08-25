# crossplane-function-sdk

A Rust SDK for writing [Crossplane](https://www.crossplane.io)
[composition functions](https://docs.crossplane.io/latest/composition/compositions/).

A composition function is a gRPC server implementing the
`FunctionRunnerService` defined by Crossplane's `apiextensions.fn.proto.v1`
API. This crate provides:

- `proto::v1` - generated protocol types with protojson serde support, plus
  the tonic gRPC server and client.
- `server` - a function-spec-compliant runtime: mTLS or insecure serving,
  gRPC server reflection, graceful shutdown, and the standard CLI arguments.
- `request`, `response`, `resource` - helpers for required resources and
  schemas, credentials, capabilities, results, conditions, and desired
  resource updates from any `serde::Serialize` source.
- `logging` - JSON-lines logging with a human-readable debug mode.

See the [repository](https://github.com/jonasz-lasut/function-sdk-rust) for a
complete example function.
