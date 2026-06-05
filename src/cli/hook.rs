use crate::cli::label::extract_tool_label;
use crate::cli::{json_str, local_time_hhmm, read_stdin_json, sanitize_value};
use crate::user_vars;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// All user variable names managed by the dashboard.
const ALL_AGENT_VARS: &[&str] = &[
    "agent_type",
    "agent_status",
    "agent_prompt",
    "agent_prompt_source",
    "agent_started_at",
    "agent_wait_reason",
    "agent_permission_mode",
    "agent_subagents",
    "agent_cwd",
    "agent_attention",
];

/// Handle a hook event from an AI agent.
/// Called as: `wezterm-agent-dashboard hook <agent> <event>`
pub fn cmd_hook(args: &[String]) -> i32 {
    let agent = match args.first() {
        Some(a) => a.as_str(),
        None => return 0,
    };
    let event = match args.get(1) {
        Some(e) => e.as_str(),
        None => return 0,
    };

    match agent {
        "claude" | "codex" => {}
        _ => return 0,
    }

    let pane_id = std::env::var("WEZTERM_PANE").unwrap_or_default();
    if pane_id.is_empty() {
        return 0;
    }

    let input = read_stdin_json();

    match event {
        "user-prompt-submit" => handle_user_prompt_submit(agent, &input),
        "notification" => handle_notification(agent, &input),
        "stop" => handle_stop(agent, &input),
        "stop-failure" => handle_stop_failure(agent, &input),
        "session-start" => handle_session_start(agent),
        "session-end" => handle_session_end(&pane_id),
        "activity-log" => handle_activity_log(&pane_id, &input),
        "subagent-start" => handle_subagent_start(&input),
        "subagent-stop" => handle_subagent_stop(&input),
        _ => {}
    }

    0
}

fn handle_user_prompt_submit(agent: &str, input: &Value) {
    let prompt = json_str(input, "prompt");
    let now = current_epoch().to_string();

    let mut vars: Vec<(&str, &str)> = vec![
        ("agent_type", agent),
        ("agent_status", "running"),
        ("agent_attention", ""),
        ("agent_started_at", &now),
        ("agent_wait_reason", ""),
    ];

    // Only update prompt if it's a real user message
    if !prompt.is_empty() && !is_system_message(prompt) {
        let sanitized = sanitize_value(prompt);
        // We need to own the sanitized string
        user_vars::set_user_vars(&vars);
        user_vars::set_user_var("agent_prompt", &sanitized);
        user_vars::set_user_var("agent_prompt_source", "user");
    } else {
        vars.push(("agent_prompt_source", "user"));
        user_vars::set_user_vars(&vars);
    }

    update_cwd_and_mode(agent, input);
}

fn handle_notification(agent: &str, input: &Value) {
    let wait_reason = json_str(input, "type");

    user_vars::set_user_vars(&[
        ("agent_type", agent),
        ("agent_status", "waiting"),
        ("agent_attention", "notification"),
        ("agent_wait_reason", wait_reason),
    ]);
}

fn handle_stop(agent: &str, input: &Value) {
    let response = json_str(input, "last_assistant_message");

    let mut vars = vec![
        ("agent_type", agent),
        ("agent_status", "idle"),
        ("agent_attention", ""),
        ("agent_wait_reason", ""),
        ("agent_subagents", ""),
    ];

    if !response.is_empty() {
        let sanitized = sanitize_value(response);
        user_vars::set_user_vars(&vars);
        user_vars::set_user_var("agent_prompt", &sanitized);
        user_vars::set_user_var("agent_prompt_source", "response");
    } else {
        vars.push(("agent_prompt_source", "response"));
        user_vars::set_user_vars(&vars);
    }
}

fn handle_stop_failure(agent: &str, input: &Value) {
    let error = json_str(input, "error");

    user_vars::set_user_vars(&[
        ("agent_type", agent),
        ("agent_status", "error"),
        ("agent_attention", ""),
        ("agent_wait_reason", ""),
    ]);

    if !error.is_empty() {
        let sanitized = sanitize_value(error);
        user_vars::set_user_var("agent_prompt", &sanitized);
        user_vars::set_user_var("agent_prompt_source", "response");
    }
}

fn handle_session_start(agent: &str) {
    user_vars::clear_user_vars(ALL_AGENT_VARS);
    user_vars::set_user_vars(&[("agent_type", agent), ("agent_status", "idle")]);
}

fn handle_session_end(pane_id: &str) {
    user_vars::clear_user_vars(ALL_AGENT_VARS);

    // Delete the activity log file
    let log_path = activity_log_path(pane_id);
    let _ = fs::remove_file(log_path);
}

fn handle_activity_log(pane_id: &str, input: &Value) {
    let tool_name = json_str(input, "tool_name");
    if tool_name.is_empty() {
        return;
    }

    let tool_input = input.get("tool_input").cloned().unwrap_or(Value::Null);
    let tool_response = input.get("tool_response").cloned().unwrap_or(Value::Null);

    // Handle permission mode changes via tool use
    match tool_name {
        "EnterPlanMode" => {
            user_vars::set_user_var("agent_permission_mode", "plan");
        }
        "ExitPlanMode" => {
            user_vars::set_user_var("agent_permission_mode", "default");
        }
        _ => {}
    }

    // Update status to running if not already
    user_vars::set_user_var("agent_status", "running");

    let label = extract_tool_label(tool_name, &tool_input, &tool_response);
    write_activity_entry(pane_id, tool_name, &label);
}

fn handle_subagent_start(input: &Value) {
    let agent_type = json_str(input, "agent_type");
    if agent_type.is_empty() {
        return;
    }

    // Read current subagents, append new one
    // Since we can't read user vars from inside the pane,
    // we use a simple file-based approach or just set the new value
    // The hook receives the full state, so we append
    let current = json_str(input, "current_subagents");
    let new_list = if current.is_empty() {
        agent_type.to_string()
    } else {
        format!("{current},{agent_type}")
    };

    user_vars::set_user_var("agent_subagents", &new_list);
}

fn handle_subagent_stop(input: &Value) {
    let agent_type = json_str(input, "agent_type");
    if agent_type.is_empty() {
        return;
    }

    let current = json_str(input, "current_subagents");
    let mut parts: Vec<&str> = current.split(',').filter(|s| !s.is_empty()).collect();

    // Remove last occurrence of this agent type
    if let Some(pos) = parts.iter().rposition(|&s| s == agent_type) {
        parts.remove(pos);
    }

    let new_list = parts.join(",");
    user_vars::set_user_var("agent_subagents", &new_list);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn update_cwd_and_mode(agent: &str, input: &Value) {
    let cwd = json_str(input, "cwd");
    if !cwd.is_empty() {
        // Don't update cwd when subagents are active
        let subagents = json_str(input, "current_subagents");
        if subagents.is_empty() {
            user_vars::set_user_var("agent_cwd", cwd);
        }
    }

    if agent == "claude" {
        let mode = json_str(input, "permission_mode");
        if !mode.is_empty() {
            user_vars::set_user_var("agent_permission_mode", mode);
        }
    }
}

fn is_system_message(prompt: &str) -> bool {
    prompt.starts_with("/") || prompt.starts_with("CTRL-") || prompt.len() < 2
}

fn current_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn activity_log_path(pane_id: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/wezterm-agent-activity-{pane_id}.log"))
}

fn write_activity_entry(pane_id: &str, tool_name: &str, label: &str) {
    let path = activity_log_path(pane_id);
    let hhmm = local_time_hhmm();

    let label = format_activity_label(label, 200);

    let entry = format!("{hhmm}|{tool_name}|{label}\n");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(entry.as_bytes());
    }

    // Trim log if too large
    trim_log_file(&path, 200, 210);
}

fn format_activity_label(label: &str, max_chars: usize) -> String {
    let sanitized = label.replace(['\n', '|'], " ");
    sanitized.chars().take(max_chars).collect()
}

fn trim_log_file(path: &PathBuf, keep: usize, threshold: usize) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= threshold {
        return;
    }

    let start = lines.len().saturating_sub(keep);
    let trimmed = lines[start..].join("\n") + "\n";
    let _ = fs::write(path, trimmed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_is_system_message_slash_command() {
        assert!(is_system_message("/help"));
        assert!(is_system_message("/exit"));
    }

    #[test]
    fn test_is_system_message_ctrl() {
        assert!(is_system_message("CTRL-C"));
        assert!(is_system_message("CTRL-D"));
    }

    #[test]
    fn test_is_system_message_short() {
        assert!(is_system_message("a"));
        assert!(is_system_message(""));
    }

    #[test]
    fn test_is_not_system_message() {
        assert!(!is_system_message("Fix the bug in main.rs"));
        assert!(!is_system_message("Add tests to the project"));
        assert!(!is_system_message("hi"));
    }

    #[test]
    fn test_activity_log_path() {
        let path = activity_log_path("42");
        assert_eq!(path, PathBuf::from("/tmp/wezterm-agent-activity-42.log"));
    }

    #[test]
    fn test_format_activity_label_truncates_on_utf8_boundary() {
        let label = "あ".repeat(100);
        let truncated = format_activity_label(&label, 67);
        assert_eq!(truncated, "あ".repeat(67));
    }

    #[test]
    fn test_format_activity_label_sanitizes_separators() {
        let label = "line1\nline2|line3";
        assert_eq!(format_activity_label(label, 200), "line1 line2 line3");
    }

    #[test]
    fn test_trim_log_file_under_threshold() {
        let dir = std::env::temp_dir();
        let path = dir.join("test-trim-under.log");
        // Write 5 lines, threshold is 10 — should not trim
        let mut f = fs::File::create(&path).unwrap();
        for i in 0..5 {
            writeln!(f, "line {i}").unwrap();
        }
        drop(f);

        trim_log_file(&path, 3, 10);

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 5);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_trim_log_file_over_threshold() {
        let dir = std::env::temp_dir();
        let path = dir.join("test-trim-over.log");
        // Write 15 lines, threshold 10, keep 5
        let mut f = fs::File::create(&path).unwrap();
        for i in 0..15 {
            writeln!(f, "line {i}").unwrap();
        }
        drop(f);

        trim_log_file(&path, 5, 10);

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "line 10");
        assert_eq!(lines[4], "line 14");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_trim_log_file_nonexistent() {
        let path = PathBuf::from("/tmp/nonexistent-trim-test-12345.log");
        // Should not panic
        trim_log_file(&path, 5, 10);
    }
}
