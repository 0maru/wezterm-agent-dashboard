use unicode_width::UnicodeWidthStr;

/// Truncate a string to fit within max_width columns, adding "…" if truncated.
pub fn truncate(s: &str, max_width: usize) -> String {
    let width = UnicodeWidthStr::width(s);
    if width <= max_width {
        return s.to_string();
    }

    if max_width == 0 {
        return String::new();
    }

    let mut result = String::new();
    let mut current_width = 0;
    let target = max_width.saturating_sub(1); // leave room for "…"

    for ch in s.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > target {
            break;
        }
        result.push(ch);
        current_width += ch_width;
    }

    result.push('…');
    result
}

/// Format elapsed time since `started_at` (epoch seconds) as a human-readable string.
pub fn format_elapsed(now: u64, started_at: u64) -> String {
    if started_at == 0 || started_at > now {
        return String::new();
    }

    let elapsed = now - started_at;

    if elapsed < 60 {
        format!("{elapsed}s")
    } else if elapsed < 3600 {
        let mins = elapsed / 60;
        let secs = elapsed % 60;
        format!("{mins}:{secs:02}")
    } else {
        let hours = elapsed / 3600;
        let mins = (elapsed % 3600) / 60;
        format!("{hours}:{mins:02}:{:02}", elapsed % 60)
    }
}

pub fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format_compact_decimal(tokens as f64 / 1_000_000.0, "M")
    } else if tokens >= 1_000 {
        format_compact_decimal(tokens as f64 / 1_000.0, "k")
    } else {
        tokens.to_string()
    }
}

pub fn format_cost_usd(cost: f64) -> String {
    if !cost.is_finite() || cost <= 0.0 {
        return "$0".to_string();
    }

    if cost < 0.01 {
        format!("${cost:.4}")
    } else {
        format!("${cost:.2}")
    }
}

/// Pad or truncate a string to exactly `width` columns.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let current = UnicodeWidthStr::width(s);
    if current >= width {
        truncate(s, width)
    } else {
        let padding = width - current;
        format!("{s}{}", " ".repeat(padding))
    }
}

fn format_compact_decimal(value: f64, suffix: &str) -> String {
    let value = format!("{value:.1}");
    let value = value.strip_suffix(".0").unwrap_or(&value);
    format!("{value}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world", 8), "hello w…");
    }

    #[test]
    fn test_truncate_zero() {
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn test_format_elapsed_seconds() {
        assert_eq!(format_elapsed(100, 70), "30s");
    }

    #[test]
    fn test_format_elapsed_minutes() {
        assert_eq!(format_elapsed(1000, 850), "2:30");
    }

    #[test]
    fn test_format_elapsed_hours() {
        assert_eq!(format_elapsed(10000, 3400), "1:50:00");
    }

    #[test]
    fn test_format_elapsed_future() {
        assert_eq!(format_elapsed(100, 200), "");
    }

    #[test]
    fn test_format_token_count() {
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1_500), "1.5k");
        assert_eq!(format_token_count(12_000), "12k");
        assert_eq!(format_token_count(1_250_000), "1.2M");
    }

    #[test]
    fn test_format_cost_usd() {
        assert_eq!(format_cost_usd(0.0), "$0");
        assert_eq!(format_cost_usd(0.00123), "$0.0012");
        assert_eq!(format_cost_usd(0.123), "$0.12");
        assert_eq!(format_cost_usd(12.345), "$12.35");
    }

    #[test]
    fn test_pad_short() {
        assert_eq!(pad_to_width("hi", 5), "hi   ");
    }

    #[test]
    fn test_pad_exact() {
        assert_eq!(pad_to_width("hello", 5), "hello");
    }
}
