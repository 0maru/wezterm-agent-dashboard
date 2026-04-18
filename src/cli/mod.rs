pub mod hook;
pub mod label;
pub mod toggle;

use std::io::Read;

/// Dispatch CLI subcommands. Returns Some(exit_code) if a subcommand matched,
/// None if the binary should launch the TUI.
pub fn run(args: &[String]) -> Option<i32> {
    let cmd = args.first().map(|s| s.as_str())?;
    let rest = &args[1..];
    let code = match cmd {
        "hook" => hook::cmd_hook(rest),
        "toggle" => toggle::cmd_toggle(rest),
        "--version" | "version" => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => return None,
    };
    Some(code)
}

// ---------------------------------------------------------------------------
// Shared helpers used by hook and other subcommands
// ---------------------------------------------------------------------------

/// Read JSON from stdin (if stdin is not a TTY).
pub fn read_stdin_json() -> serde_json::Value {
    use std::io::IsTerminal;
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return serde_json::Value::Null;
    }

    let mut buf = String::new();
    if stdin.lock().read_to_string(&mut buf).is_err() {
        return serde_json::Value::Null;
    }

    serde_json::from_str(&buf).unwrap_or(serde_json::Value::Null)
}

/// Safely extract a string field from a JSON value.
pub fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Get local time as HH:MM string.
pub fn local_time_hhmm() -> String {
    let format = time::macros::format_description!("[hour]:[minute]");

    if let Ok(now) = time::OffsetDateTime::now_local()
        && let Ok(formatted) = now.format(&format)
    {
        return formatted;
    }

    time::OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_else(|_| "00:00".to_string())
}

/// Sanitize a string value for storage: replace newlines and pipes with spaces.
pub fn sanitize_value(s: &str) -> String {
    s.replace(['\n', '\r', '|'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_value_newlines() {
        assert_eq!(sanitize_value("hello\nworld"), "hello world");
    }

    #[test]
    fn test_sanitize_value_carriage_return() {
        assert_eq!(sanitize_value("hello\r\nworld"), "hello  world");
    }

    #[test]
    fn test_sanitize_value_pipes() {
        assert_eq!(sanitize_value("a|b|c"), "a b c");
    }

    #[test]
    fn test_sanitize_value_clean() {
        assert_eq!(sanitize_value("hello world"), "hello world");
    }

    #[test]
    fn test_json_str_existing_key() {
        let val = serde_json::json!({"name": "test", "count": 42});
        assert_eq!(json_str(&val, "name"), "test");
    }

    #[test]
    fn test_json_str_missing_key() {
        let val = serde_json::json!({"name": "test"});
        assert_eq!(json_str(&val, "missing"), "");
    }

    #[test]
    fn test_json_str_non_string_value() {
        let val = serde_json::json!({"count": 42});
        assert_eq!(json_str(&val, "count"), "");
    }

    #[test]
    fn test_json_str_null() {
        let val = serde_json::Value::Null;
        assert_eq!(json_str(&val, "anything"), "");
    }
}
