use crate::cli::label::extract_tool_label;
use crate::cli::{json_str, local_time_hhmm, read_stdin_json, sanitize_value};
use crate::user_vars;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
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
        "stop" => handle_stop(&pane_id, agent, &input),
        "stop-failure" => handle_stop_failure(agent, &input),
        "session-start" => handle_session_start(&pane_id, agent),
        "session-end" => handle_session_end(&pane_id),
        "activity-log" => handle_activity_log(&pane_id, &input),
        "subagent-start" => handle_subagent_start(&pane_id, &input),
        "subagent-stop" => handle_subagent_stop(&pane_id, &input),
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

fn handle_stop(pane_id: &str, agent: &str, input: &Value) {
    let response = json_str(input, "last_assistant_message");
    let _ = fs::remove_file(subagent_state_path(pane_id));

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

fn handle_session_start(pane_id: &str, agent: &str) {
    let _ = fs::remove_file(subagent_state_path(pane_id));

    user_vars::clear_user_vars(ALL_AGENT_VARS);
    user_vars::set_user_vars(&[("agent_type", agent), ("agent_status", "idle")]);
}

fn handle_session_end(pane_id: &str) {
    user_vars::clear_user_vars(ALL_AGENT_VARS);

    // Delete the activity log file
    let log_path = activity_log_path(pane_id);
    let _ = fs::remove_file(log_path);
    let _ = fs::remove_file(subagent_state_path(pane_id));
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

fn handle_subagent_start(pane_id: &str, input: &Value) {
    let Some(entry) = subagent_entry_from_input(input) else {
        return;
    };

    let mut entries = load_subagent_entries(pane_id, input);
    upsert_subagent_entry(&mut entries, entry);

    persist_subagent_entries(pane_id, &entries);
}

fn handle_subagent_stop(pane_id: &str, input: &Value) {
    let mut entries = load_subagent_entries(pane_id, input);
    remove_subagent_entry(&mut entries, input);
    persist_subagent_entries(pane_id, &entries);
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SubagentEntry {
    id: String,
    label: String,
}

fn subagent_state_path(pane_id: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/wezterm-agent-subagents-{pane_id}.json"))
}

fn load_subagent_entries(pane_id: &str, input: &Value) -> Vec<SubagentEntry> {
    read_subagent_entries(&subagent_state_path(pane_id))
        .unwrap_or_else(|| legacy_subagent_entries(input))
}

fn read_subagent_entries(path: &Path) -> Option<Vec<SubagentEntry>> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn persist_subagent_entries(pane_id: &str, entries: &[SubagentEntry]) {
    let path = subagent_state_path(pane_id);
    if entries.is_empty() {
        let _ = fs::remove_file(path);
    } else if let Ok(content) = serde_json::to_string(entries) {
        let _ = fs::write(path, content);
    }

    user_vars::set_user_var("agent_subagents", &subagent_labels(entries));
}

fn subagent_entry_from_input(input: &Value) -> Option<SubagentEntry> {
    let id = subagent_id(input);
    let label = subagent_label(input).or_else(|| id.clone())?;
    let id = id.unwrap_or_else(|| format!("legacy:{}", current_epoch_nanos()));

    Some(SubagentEntry { id, label })
}

fn upsert_subagent_entry(entries: &mut Vec<SubagentEntry>, entry: SubagentEntry) {
    if let Some(pos) = entries.iter().position(|current| current.id == entry.id) {
        entries[pos] = entry;
    } else {
        entries.push(entry);
    }
}

fn remove_subagent_entry(entries: &mut Vec<SubagentEntry>, input: &Value) {
    if let Some(id) = subagent_id(input) {
        entries.retain(|entry| entry.id != id);
        return;
    }

    if let Some(label) = subagent_label(input).as_deref()
        && let Some(pos) = entries.iter().rposition(|entry| entry.label == label)
    {
        entries.remove(pos);
    }
}

fn subagent_id(input: &Value) -> Option<String> {
    first_json_string(input, &["agent_id", "task_id", "id"])
}

fn subagent_label(input: &Value) -> Option<String> {
    first_json_string(input, &["agent_type", "nickname", "name", "teammate_name"])
        .map(|label| sanitize_subagent_label(&label))
        .filter(|label| !label.is_empty())
}

fn first_json_string(input: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| input.get(*key).and_then(Value::as_str))
        .find(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn legacy_subagent_entries(input: &Value) -> Vec<SubagentEntry> {
    json_str(input, "current_subagents")
        .split(',')
        .map(sanitize_subagent_label)
        .filter(|label| !label.is_empty())
        .enumerate()
        .map(|(idx, label)| SubagentEntry {
            id: format!("legacy:{idx}:{label}"),
            label,
        })
        .collect()
}

fn subagent_labels(entries: &[SubagentEntry]) -> String {
    entries
        .iter()
        .map(|entry| entry.label.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn sanitize_subagent_label(label: &str) -> String {
    sanitize_value(label).replace(',', " ")
}

fn current_epoch_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn write_activity_entry(pane_id: &str, tool_name: &str, label: &str) {
    let path = activity_log_path(pane_id);
    let hhmm = local_time_hhmm();

    // Truncate label for display
    let label = if label.len() > 200 {
        &label[..200]
    } else {
        label
    };

    // Replace newlines and pipes in label
    let label = label.replace(['\n', '|'], " ");

    let entry = format!("{hhmm}|{tool_name}|{label}\n");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(entry.as_bytes());
    }

    // Trim log if too large
    trim_log_file(&path, 200, 210);
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
    fn test_subagent_entry_from_input_uses_agent_id_and_type() {
        let input = serde_json::json!({
            "agent_id": "agent-abc",
            "agent_type": "Explore"
        });

        let entry = subagent_entry_from_input(&input).unwrap();

        assert_eq!(
            entry,
            SubagentEntry {
                id: "agent-abc".into(),
                label: "Explore".into(),
            }
        );
    }

    #[test]
    fn test_legacy_subagent_entries_from_current_subagents() {
        let input = serde_json::json!({
            "current_subagents": "Explore,Review\nAgent,Plan|Mode"
        });

        let entries = legacy_subagent_entries(&input);

        assert_eq!(subagent_labels(&entries), "Explore,Review Agent,Plan Mode");
    }

    #[test]
    fn test_subagent_state_roundtrip() {
        let path = std::env::temp_dir().join(format!(
            "wezterm-agent-dashboard-subagents-{}-{}.json",
            std::process::id(),
            current_epoch_nanos()
        ));
        let entries = vec![
            SubagentEntry {
                id: "agent-a".into(),
                label: "Explore".into(),
            },
            SubagentEntry {
                id: "agent-b".into(),
                label: "Review".into(),
            },
        ];
        fs::write(&path, serde_json::to_string(&entries).unwrap()).unwrap();

        let entries = read_subagent_entries(&path).unwrap();
        assert_eq!(subagent_labels(&entries), "Explore,Review");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_subagent_upsert_replaces_duplicate_id() {
        let mut entries = vec![SubagentEntry {
            id: "agent-a".into(),
            label: "Explore".into(),
        }];

        upsert_subagent_entry(
            &mut entries,
            SubagentEntry {
                id: "agent-a".into(),
                label: "Review".into(),
            },
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(subagent_labels(&entries), "Review");
    }

    #[test]
    fn test_subagent_remove_by_id() {
        let mut entries = vec![
            SubagentEntry {
                id: "a".into(),
                label: "Explore".into(),
            },
            SubagentEntry {
                id: "b".into(),
                label: "Explore".into(),
            },
            SubagentEntry {
                id: "c".into(),
                label: "Plan".into(),
            },
        ];

        remove_subagent_entry(
            &mut entries,
            &serde_json::json!({"agent_id": "b", "agent_type": "Explore"}),
        );

        assert_eq!(subagent_labels(&entries), "Explore,Plan");
        assert_eq!(entries[0].id, "a");
    }

    #[test]
    fn test_subagent_unknown_id_does_not_remove_by_label() {
        let mut entries = vec![SubagentEntry {
            id: "a".into(),
            label: "Explore".into(),
        }];

        remove_subagent_entry(
            &mut entries,
            &serde_json::json!({"agent_id": "missing", "agent_type": "Explore"}),
        );

        assert_eq!(subagent_labels(&entries), "Explore");
    }

    #[test]
    fn test_subagent_remove_without_id_removes_last_matching_label() {
        let mut entries = vec![
            SubagentEntry {
                id: "a".into(),
                label: "Explore".into(),
            },
            SubagentEntry {
                id: "b".into(),
                label: "Explore".into(),
            },
            SubagentEntry {
                id: "c".into(),
                label: "Plan".into(),
            },
        ];

        remove_subagent_entry(&mut entries, &serde_json::json!({"agent_type": "Explore"}));

        assert_eq!(subagent_labels(&entries), "Explore,Plan");
        assert_eq!(entries[0].id, "a");
    }

    #[test]
    fn test_subagent_stop_falls_back_to_current_subagents() {
        let input = serde_json::json!({
            "agent_type": "Review",
            "current_subagents": "Explore,Review"
        });
        let mut entries = legacy_subagent_entries(&input);

        remove_subagent_entry(&mut entries, &input);

        assert_eq!(subagent_labels(&entries), "Explore");
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
