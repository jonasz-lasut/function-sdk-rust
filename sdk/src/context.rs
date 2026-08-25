//! Well-known function context keys.
//!
//! Context is a scratch key-value area threaded through every function call
//! in a pipeline. Read keys with [`crate::request::get_context_key`] and set
//! them with [`crate::response::set_context_key`].

/// The context key Crossplane sets to inject the composition environment
/// into function context.
///
/// This is an alpha Crossplane feature. It is not honored unless the
/// relevant Crossplane feature flag is enabled, and may change or be removed
/// without notice.
pub const KEY_ENVIRONMENT: &str = "apiextensions.crossplane.io/environment";
