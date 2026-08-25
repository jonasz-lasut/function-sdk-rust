//! Helpers for working with RunFunctionResponses.

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;

use crate::Error;
use crate::proto::v1::{
    Condition, MatchLabels, Requirements, ResourceSelector, Result as FnResult, RunFunctionRequest,
    RunFunctionResponse, SchemaSelector, Severity, Status, resource_selector,
};
use crate::resource::json_to_struct;

/// The default TTL for which a RunFunctionResponse may be cached.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);

/// Creates a response to the supplied request.
///
/// The request's tag, desired state, and context are copied to the response.
/// Crossplane deletes any previously desired field or resource that is not
/// copied forward, so always start from this function rather than from an
/// empty response.
pub fn to(req: &RunFunctionRequest, ttl: Duration) -> RunFunctionResponse {
    RunFunctionResponse {
        meta: Some(crate::proto::v1::ResponseMeta {
            tag: req.meta.as_ref().map(|m| m.tag.clone()).unwrap_or_default(),
            ttl: Some(pbjson_types::Duration {
                seconds: ttl.as_secs() as i64,
                nanos: ttl.subsec_nanos() as i32,
            }),
        }),
        desired: req.desired.clone(),
        context: req.context.clone(),
        ..Default::default()
    }
}

/// Adds a normal result to the response.
pub fn normal(rsp: &mut RunFunctionResponse, message: impl Into<String>) {
    add_result(rsp, Severity::Normal, message);
}

/// Adds a warning result to the response.
pub fn warning(rsp: &mut RunFunctionResponse, message: impl Into<String>) {
    add_result(rsp, Severity::Warning, message);
}

/// Adds a fatal result to the response.
///
/// The pipeline run is considered a failure and the first fatal result is
/// returned as an error, but subsequent pipeline steps may still run.
pub fn fatal(rsp: &mut RunFunctionResponse, message: impl Into<String>) {
    add_result(rsp, Severity::Fatal, message);
}

fn add_result(rsp: &mut RunFunctionResponse, severity: Severity, message: impl Into<String>) {
    rsp.results.push(FnResult {
        severity: severity as i32,
        message: message.into(),
        reason: None,
        target: None,
    });
}

/// Adds a True status condition to be applied to the XR.
///
/// Returns the added condition so a message or target can be set on it. Do
/// not set the Ready condition type; Crossplane manages it from resource
/// readiness.
pub fn condition_true(
    rsp: &mut RunFunctionResponse,
    typ: impl Into<String>,
    reason: impl Into<String>,
) -> &mut Condition {
    add_condition(rsp, Status::ConditionTrue, typ, reason)
}

/// Adds a False status condition to be applied to the XR.
pub fn condition_false(
    rsp: &mut RunFunctionResponse,
    typ: impl Into<String>,
    reason: impl Into<String>,
) -> &mut Condition {
    add_condition(rsp, Status::ConditionFalse, typ, reason)
}

/// Adds an Unknown status condition to be applied to the XR.
pub fn condition_unknown(
    rsp: &mut RunFunctionResponse,
    typ: impl Into<String>,
    reason: impl Into<String>,
) -> &mut Condition {
    add_condition(rsp, Status::ConditionUnknown, typ, reason)
}

fn add_condition(
    rsp: &mut RunFunctionResponse,
    status: Status,
    typ: impl Into<String>,
    reason: impl Into<String>,
) -> &mut Condition {
    rsp.conditions.push(Condition {
        r#type: typ.into(),
        status: status as i32,
        reason: reason.into(),
        message: None,
        target: None,
    });
    rsp.conditions
        .last_mut()
        .expect("a condition was just pushed")
}

/// How a resource requirement matches resources.
#[derive(Clone, Debug)]
pub enum ResourceMatch {
    /// Match the resource with this name.
    Name(String),
    /// Match all resources with these labels.
    Labels(HashMap<String, String>),
}

/// Adds a resource requirement to the response.
///
/// Crossplane fetches the matching resources and calls the function again
/// with them in `req.required_resources[name]`. Matching None selects all
/// resources of the given API version and kind. Omit the namespace to match
/// cluster scoped resources, or to match namespaced resources by labels
/// across all namespaces.
pub fn require_resources(
    rsp: &mut RunFunctionResponse,
    name: impl Into<String>,
    api_version: impl Into<String>,
    kind: impl Into<String>,
    r#match: Option<ResourceMatch>,
    namespace: Option<String>,
) {
    let selector = ResourceSelector {
        api_version: api_version.into(),
        kind: kind.into(),
        r#match: r#match.map(|m| match m {
            ResourceMatch::Name(name) => resource_selector::Match::MatchName(name),
            ResourceMatch::Labels(labels) => {
                resource_selector::Match::MatchLabels(MatchLabels { labels })
            }
        }),
        namespace,
    };
    requirements(rsp).resources.insert(name.into(), selector);
}

/// Adds a schema requirement to the response.
///
/// Crossplane fetches the OpenAPI v3 schema for the resource kind and calls
/// the function again with it in `req.required_schemas[name]`. Read it with
/// [`crate::request::get_required_schema`].
pub fn require_schema(
    rsp: &mut RunFunctionResponse,
    name: impl Into<String>,
    api_version: impl Into<String>,
    kind: impl Into<String>,
) {
    let selector = SchemaSelector {
        api_version: api_version.into(),
        kind: kind.into(),
    };
    requirements(rsp).schemas.insert(name.into(), selector);
}

fn requirements(rsp: &mut RunFunctionResponse) -> &mut Requirements {
    rsp.requirements.get_or_insert_default()
}

/// Sets the output of an operation function.
///
/// The source must serialize to a JSON object. Output is written to the
/// Operation's status.pipeline field; XRs discard function output.
pub fn set_output<T: Serialize + ?Sized>(
    rsp: &mut RunFunctionResponse,
    output: &T,
) -> Result<(), Error> {
    let value = serde_json::to_value(output)?;
    let serde_json::Value::Object(map) = value else {
        return Err(Error::NotAnObject);
    };
    rsp.output = Some(json_to_struct(&map));
    Ok(())
}
