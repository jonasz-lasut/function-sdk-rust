//! Logging setup for composition functions.

/// Configures process-wide logging.
///
/// Logs are emitted as JSON lines at info level. With debug enabled, logs
/// are emitted in a human-readable format at debug level instead. Call once,
/// before serving.
pub fn configure(debug: bool) {
    if debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_file(true)
            .with_line_number(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_file(true)
            .with_line_number(true)
            .init();
    }
}
