//! Pure helpers for rendering download metadata in user-facing strings.
//!
//! Base 1024 for bytes (industry standard for download managers), one
//! decimal place from KiB upwards. Duration drops zero leading components
//! ("45s", "1m 5s", "2h 30m"). Domain-pure: std-only, no allocator-heavy
//! crates such as `humansize` (avoids dependency creep for trivial logic).

/// Format a byte count as a short human-readable string.
///
/// Uses base 1024 (KB = 1024 B) and one decimal once the unit is at
/// least KB. Bytes (`B`) stay integer. Always `≥ 0` (input is `u64`).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    format!("{value:.1} {}", UNITS[idx])
}

/// Format a transfer rate (bytes per second) by appending "/s".
pub fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_size(bytes_per_sec))
}

/// Format a duration as compact "Hh Mm Ss".
///
/// Leading zero components are dropped: `45` → "45s", `60` → "1m",
/// `125` → "2m 5s", `3700` → "1h 1m 40s". Pure, std-only.
pub fn format_duration(secs: u64) -> String {
    if secs == 0 {
        return "0s".to_string();
    }
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 {
        parts.push(format!("{seconds}s"));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_zero_bytes_renders_as_b_unit() {
        assert_eq!(format_size(0), "0 B");
    }

    #[test]
    fn test_format_size_below_kib_keeps_byte_unit() {
        assert_eq!(format_size(1), "1 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_exactly_one_kib_uses_kb_unit() {
        assert_eq!(format_size(1024), "1.0 KB");
    }

    #[test]
    fn test_format_size_megabytes_uses_one_decimal() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(2_500_000), "2.4 MB");
    }

    #[test]
    fn test_format_size_gigabytes_threshold() {
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_format_size_terabytes_caps_at_tb() {
        // 5 TiB
        let bytes = 5_u64 * 1024_u64.pow(4);
        assert_eq!(format_size(bytes), "5.0 TB");
    }

    #[test]
    fn test_format_size_huge_value_does_not_overflow_unit_index() {
        // Many petabytes — must stay capped at TB, not panic.
        assert!(format_size(u64::MAX).ends_with(" TB"));
    }

    #[test]
    fn test_format_speed_appends_per_second_suffix() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(1024), "1.0 KB/s");
        assert_eq!(format_speed(1_500_000), "1.4 MB/s");
    }

    #[test]
    fn test_format_duration_zero_returns_zero_seconds() {
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn test_format_duration_seconds_only() {
        assert_eq!(format_duration(1), "1s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_drops_zero_seconds() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(3600), "1h");
    }

    #[test]
    fn test_format_duration_combines_minutes_and_seconds() {
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn test_format_duration_combines_hours_minutes_seconds() {
        assert_eq!(format_duration(3700), "1h 1m 40s");
    }

    #[test]
    fn test_format_duration_skips_middle_zero_minute_component() {
        // 1h plus 5 seconds — minutes part is 0 and must be omitted.
        assert_eq!(format_duration(3605), "1h 5s");
    }
}
