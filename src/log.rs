//! Structured logging — human-readable to stderr.

/// Emit a warning-level log line to stderr.
/// Automatically flushes to ensure journald captures each line immediately.
pub(crate) fn warn(msg: &str) {
    eprintln!("⚠️ {msg}");
    use std::io::Write;
    let _ = std::io::stderr().flush();
}

/// Log a warning-level message.
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::log::warn(&format!($($arg)*));
    };
}
