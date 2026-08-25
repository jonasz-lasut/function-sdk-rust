use std::time::Duration;

use crossplane_function_sdk::proto::v1::{
    Capability, RunFunctionRequest, RunFunctionResponse, Severity,
};
use crossplane_function_sdk::{request, resource, response};

fn fixture() -> RunFunctionRequest {
    serde_json::from_str(
        r#"{
            "meta": {"tag": "test-tag", "capabilities": ["CAPABILITY_CAPABILITIES", "CAPABILITY_CONDITIONS"]},
            "observed": {
                "composite": {
                    "resource": {
                        "apiVersion": "example.org/v1",
                        "kind": "XR",
                        "metadata": {"name": "my-xr"},
                        "spec": {"region": "eu-central-1"},
                        "status": {
                            "conditions": [
                                {"type": "Ready", "status": "True", "reason": "Available"}
                            ]
                        }
                    }
                }
            },
            "desired": {
                "resources": {
                    "bucket": {"resource": {"apiVersion": "s3.aws.upbound.io/v1beta2", "kind": "Bucket"}}
                }
            },
            "input": {"version": "v1beta2"},
            "context": {"apiextensions.crossplane.io/environment": {"key": "value"}}
        }"#,
    )
    .expect("fixture must be valid protojson")
}

#[test]
fn request_parses_from_protojson() {
    let req = fixture();
    assert_eq!(req.meta.as_ref().unwrap().tag, "test-tag");
    assert!(request::advertises_capabilities(&req));
    assert!(request::has_capability(&req, Capability::Conditions));
    assert!(!request::has_capability(&req, Capability::RequiredSchemas));
}

#[test]
fn response_to_copies_tag_desired_and_context() {
    let req = fixture();
    let rsp = response::to(&req, response::DEFAULT_TTL);

    assert_eq!(rsp.meta.as_ref().unwrap().tag, "test-tag");
    assert_eq!(rsp.meta.as_ref().unwrap().ttl.as_ref().unwrap().seconds, 60);
    assert_eq!(rsp.desired, req.desired);
    assert_eq!(rsp.context, req.context);
}

#[test]
fn results_carry_severity() {
    let mut rsp = RunFunctionResponse::default();
    response::normal(&mut rsp, "all good");
    response::fatal(&mut rsp, "boom");

    assert_eq!(rsp.results.len(), 2);
    assert_eq!(rsp.results[0].severity, Severity::Normal as i32);
    assert_eq!(rsp.results[1].severity, Severity::Fatal as i32);
    assert_eq!(rsp.results[1].message, "boom");
}

#[test]
fn update_overwrites_and_adds_top_level_fields() {
    let req = fixture();
    let mut rsp = response::to(&req, response::DEFAULT_TTL);
    let bucket = rsp
        .desired
        .as_mut()
        .unwrap()
        .resources
        .get_mut("bucket")
        .unwrap();

    resource::update(
        bucket,
        &serde_json::json!({
            "kind": "Bucket",
            "spec": {"forProvider": {"region": "eu-central-1"}},
        }),
    )
    .unwrap();

    let got = resource::struct_to_json(bucket.resource.as_ref().unwrap());
    assert_eq!(got["apiVersion"], "s3.aws.upbound.io/v1beta2");
    assert_eq!(got["spec"]["forProvider"]["region"], "eu-central-1");
}

#[test]
fn update_rejects_non_objects() {
    let mut r = crossplane_function_sdk::proto::v1::Resource::default();
    assert!(resource::update(&mut r, &serde_json::json!(42)).is_err());
}

#[test]
fn get_condition_reads_observed_conditions() {
    let req = fixture();
    let composite = req.observed.as_ref().unwrap().composite.as_ref();

    let ready = resource::get_condition(composite, "Ready");
    assert_eq!(ready.status, "True");
    assert_eq!(ready.reason.as_deref(), Some("Available"));

    let synced = resource::get_condition(composite, "Synced");
    assert_eq!(synced.status, "Unknown");

    let missing = resource::get_condition(None, "Ready");
    assert_eq!(missing.status, "Unknown");
}

#[test]
fn required_resources_distinguish_unresolved_from_empty() {
    let req = fixture();
    assert_eq!(request::get_required_resources(&req, "nope"), None);

    let req: RunFunctionRequest =
        serde_json::from_str(r#"{"requiredResources": {"empty": {}}}"#).unwrap();
    assert_eq!(request::get_required_resources(&req, "empty"), Some(vec![]));
}

#[test]
fn require_resources_builds_selectors() {
    let mut rsp = RunFunctionResponse::default();
    response::require_resources(
        &mut rsp,
        "vpcs",
        "ec2.aws.upbound.io/v1beta1",
        "VPC",
        Some(response::ResourceMatch::Name("my-vpc".to_string())),
        None,
    );

    let selector = &rsp.requirements.as_ref().unwrap().resources["vpcs"];
    assert_eq!(selector.kind, "VPC");
}

#[test]
fn conditions_can_set_message_and_target() {
    let mut rsp = RunFunctionResponse::default();
    let c = response::condition_true(&mut rsp, "DatabaseReady", "Available");
    c.message = Some("all databases up".to_string());

    assert_eq!(rsp.conditions.len(), 1);
    assert_eq!(rsp.conditions[0].r#type, "DatabaseReady");
    assert_eq!(
        rsp.conditions[0].message.as_deref(),
        Some("all databases up")
    );
}

#[test]
fn child_name_is_deterministic_and_dns_safe() {
    let a = resource::child_name(&["my-xr", "bucket"], "-");
    let b = resource::child_name(&["my-xr", "bucket"], "-");
    assert_eq!(a, b);
    assert!(a.starts_with("my-xr-bucket-"));
    assert_eq!(a.len(), "my-xr-bucket-".len() + 5);

    let long = "a".repeat(100);
    let name = resource::child_name(&[&long, "suffix"], "-");
    assert!(name.len() <= 63);
}

#[test]
fn struct_json_round_trip() {
    let json = serde_json::json!({
        "string": "value",
        "float": 1.5,
        "int": 3,
        "bool": true,
        "null": null,
        "list": [1, "two", {"three": 3}],
        "nested": {"deep": {"deeper": "value"}},
    });
    let serde_json::Value::Object(map) = &json else {
        unreachable!()
    };
    let s = resource::json_to_struct(map);
    assert_eq!(resource::struct_to_json(&s), json);
}

#[test]
fn observed_integers_deserialize_into_typed_structs() {
    #[derive(serde::Deserialize)]
    struct Spec {
        replicas: i64,
    }

    // Crossplane converts resources through structpb, which stores all
    // numbers as f64: replicas: 3 arrives as 3.0 on the wire.
    let req: RunFunctionRequest = serde_json::from_str(
        r#"{"observed": {"composite": {"resource": {"spec": {"replicas": 3.0}}}}}"#,
    )
    .unwrap();

    let xr = req.observed.unwrap().composite.unwrap().resource.unwrap();
    let spec: Spec = serde_json::from_value(resource::struct_to_json(&xr)["spec"].clone()).unwrap();
    assert_eq!(spec.replicas, 3);
}

#[test]
fn response_serializes_to_protojson() {
    let req = fixture();
    let mut rsp = response::to(&req, Duration::from_secs(90));
    response::normal(&mut rsp, "created bucket");

    let json = serde_json::to_value(&rsp).unwrap();
    assert_eq!(json["meta"]["tag"], "test-tag");
    assert_eq!(json["meta"]["ttl"], "90s");
    assert_eq!(json["results"][0]["severity"], "SEVERITY_NORMAL");
}
