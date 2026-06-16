//! Human-friendly print helpers shared across dracon-sync commands.
//!
//! Goal: keep CLI output consistent across the binary without pulling in a
//! colour/formatting dependency. Everything is stdlib + `comfy-table` (which
//! is already a dependency for the table-style commands).
//!
//! Conventions:
//!   - `format_bytes(n)`     → "50 MiB" / "1.4 GiB" / "512 B"
//!   - `format_secs(n)`      → "5s" / "2m 10s" / "1h 5m" / "3d 4h"
//!   - `should_color()`      → false if `NO_COLOR` is set or stdout is not a tty
//!
//! NO_COLOR spec: <https://no-color.org/> — if the env var is set (to anything,
//! including empty), colour MUST be disabled.

/// Format a byte count as a human-readable string (binary units, 2 decimals max).
pub fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = n as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", n, UNITS[0])
    } else if value >= 100.0 {
        format!("{:.0} {}", value, UNITS[unit_idx])
    } else if value >= 10.0 {
        format!("{:.1} {}", value, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", value, UNITS[unit_idx])
    }
}

/// Format a duration in seconds as a compact human-readable string.
/// Negative or zero values render as "0s".
pub fn format_secs(secs: u64) -> String {
    if secs < 60 {
        return format!("{}s", secs);
    }
    if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            return format!("{}m", m);
        }
        return format!("{}m {}s", m, s);
    }
    if secs < 86_400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            return format!("{}h", h);
        }
        return format!("{}h {}m", h, m);
    }
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    if h == 0 {
        return format!("{}d", d);
    }
    format!("{}d {}h", d, h)
}

/// Should ANSI colour codes be emitted? Honours the `NO_COLOR` env var and tty detection.
pub fn should_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("DRACON_FORCE_COLOR").is_some() {
        return true;
    }
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_zero() {
        assert_eq!(format_bytes(0), "0 B");
    }

    #[test]
    fn format_bytes_under_kib() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn format_bytes_kib() {
        assert_eq!(format_bytes(1024), "1.00 KiB");
        assert_eq!(format_bytes(15_728), "15.4 KiB");
    }

    #[test]
    fn format_bytes_mib() {
        // 52_428_800 is the example policy default (max_stage_file_bytes = 50 MiB)
        assert_eq!(format_bytes(52_428_800), "50.0 MiB");
        assert_eq!(format_bytes(1_572_864), "1.50 MiB");
    }

    #[test]
    fn format_bytes_gib() {
        // 1 GiB exactly
        assert_eq!(format_bytes(1_073_741_824), "1.00 GiB");
        // 4.5 GiB
        assert_eq!(format_bytes(4_831_838_208), "4.50 GiB");
    }

    #[test]
    fn format_secs_units() {
        assert_eq!(format_secs(0), "0s");
        assert_eq!(format_secs(5), "5s");
        assert_eq!(format_secs(59), "59s");
        assert_eq!(format_secs(60), "1m");
        assert_eq!(format_secs(130), "2m 10s");
        assert_eq!(format_secs(3600), "1h");
        assert_eq!(format_secs(3900), "1h 5m");
        assert_eq!(format_secs(86_400), "1d");
        assert_eq!(format_secs(90_000), "1d 1h");
    }

    #[test]
    fn no_color_disables() {
        // SAFETY: test runs single-threaded for these env var calls in practice.
        // The actual binary uses should_color() per-call.
        let saved = std::env::var_os("NO_COLOR");
        // This test owns the NO_COLOR slot and runs with serial tests.
        std::env::set_var("NO_COLOR", "1");
        assert!(!should_color());
        match saved {
            Some(v) => std::env::set_var("NO_COLOR", v),
            None => std::env::remove_var("NO_COLOR"),
        }
    }
}
