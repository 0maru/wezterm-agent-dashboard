use base64::Engine;
use std::io::{self, Write};

/// Set a WezTerm user variable on the current pane via OSC 1337.
///
/// The escape sequence is written to stdout, which WezTerm intercepts
/// and stores as a user variable for the pane. This is invisible to the user.
pub fn set_user_var(name: &str, value: &str) {
    let encoded = base64::engine::general_purpose::STANDARD.encode(value);
    let mut stdout = io::stdout().lock();
    let _ = write!(stdout, "\x1b]1337;SetUserVar={}={}\x07", name, encoded);
    let _ = stdout.flush();
}

/// Set multiple user variables in a single flush for efficiency.
pub fn set_user_vars(vars: &[(&str, &str)]) {
    let mut stdout = io::stdout().lock();
    for (name, value) in vars {
        let encoded = base64::engine::general_purpose::STANDARD.encode(value);
        let _ = write!(stdout, "\x1b]1337;SetUserVar={}={}\x07", name, encoded);
    }
    let _ = stdout.flush();
}

/// Clear a user variable by setting it to an empty value.
pub fn clear_user_var(name: &str) {
    set_user_var(name, "");
}

/// Clear multiple user variables at once.
pub fn clear_user_vars(names: &[&str]) {
    let vars: Vec<(&str, &str)> = names.iter().map(|n| (*n, "")).collect();
    set_user_vars(&vars);
}

/// Decode a base64-encoded user variable value.
pub fn decode_user_var(base64_value: &str) -> String {
    base64::engine::general_purpose::STANDARD
        .decode(base64_value)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_user_var() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("hello");
        assert_eq!(decode_user_var(&encoded), "hello");
    }

    #[test]
    fn test_decode_empty() {
        assert_eq!(decode_user_var(""), "");
    }

    #[test]
    fn test_decode_invalid_base64() {
        assert_eq!(decode_user_var("not-valid-base64!!!"), "");
    }

    #[test]
    fn test_decode_non_utf8() {
        let encoded = base64::engine::general_purpose::STANDARD.encode([0xff, 0xfe]);
        assert_eq!(decode_user_var(&encoded), "");
    }
}
