//! Helpers for working with RunFunctionRequests.

use crate::proto::v1::{Capability, CredentialData, RunFunctionRequest, credentials};
use crate::resource::struct_to_json;

/// The requirement name a WatchOperation uses to inject the watched resource.
pub const WATCHED_RESOURCE_KEY: &str = "ops.crossplane.io/watched-resource";

/// Gets required resources by requirement name from the request.
///
/// Returns None when Crossplane has not (yet) resolved the requirement, and
/// an empty Vec when it resolved the requirement but found no matches.
/// Always declare requirements with [`crate::response::require_resources`];
/// Crossplane considers them satisfied when they stabilize across calls.
pub fn get_required_resources(
    req: &RunFunctionRequest,
    name: &str,
) -> Option<Vec<serde_json::Value>> {
    let resources = req.required_resources.get(name)?;
    Some(
        resources
            .items
            .iter()
            .filter_map(|r| r.resource.as_ref())
            .map(struct_to_json)
            .collect(),
    )
}

/// Gets a single required resource by requirement name from the request.
///
/// A convenience for requirements that match exactly one resource. Returns
/// None when the requirement is unresolved or matched nothing.
pub fn get_required_resource(req: &RunFunctionRequest, name: &str) -> Option<serde_json::Value> {
    get_required_resources(req, name)?.into_iter().next()
}

/// Gets the watched resource that triggered this operation, if any.
///
/// When a WatchOperation creates an Operation it injects the resource that
/// changed under the requirement name [`WATCHED_RESOURCE_KEY`].
pub fn get_watched_resource(req: &RunFunctionRequest) -> Option<serde_json::Value> {
    get_required_resource(req, WATCHED_RESOURCE_KEY)
}

/// Gets the supplied credential data from the request, if any.
pub fn get_credential_data<'a>(
    req: &'a RunFunctionRequest,
    name: &str,
) -> Option<&'a CredentialData> {
    match req.credentials.get(name)?.source.as_ref()? {
        credentials::Source::CredentialData(data) => Some(data),
    }
}

/// Checks whether Crossplane advertises its capabilities at all.
///
/// Crossplane v2.2 and later advertise capabilities in the request metadata.
/// If this returns false the calling Crossplane predates capability
/// advertisement, and [`has_capability`] returns false even for features
/// that older Crossplane does support.
pub fn advertises_capabilities(req: &RunFunctionRequest) -> bool {
    has_capability(req, Capability::Capabilities)
}

/// Checks whether Crossplane advertises a particular capability.
pub fn has_capability(req: &RunFunctionRequest, cap: Capability) -> bool {
    req.meta
        .as_ref()
        .is_some_and(|m| m.capabilities().any(|c| c == cap))
}

/// Gets a required OpenAPI v3 schema by requirement name from the request.
///
/// Returns None both when the requirement is unresolved and when Crossplane
/// resolved it but found no schema. To distinguish the two, check
/// `req.required_schemas.contains_key(name)`.
pub fn get_required_schema(req: &RunFunctionRequest, name: &str) -> Option<serde_json::Value> {
    req.required_schemas
        .get(name)?
        .openapi_v3
        .as_ref()
        .map(struct_to_json)
}
