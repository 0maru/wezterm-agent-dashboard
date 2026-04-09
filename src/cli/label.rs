use serde_json::Value;

/// Extract a human-readable label from a tool invocation for display in the activity log.
///
/// Maps tool names to their most relevant input field:
/// - File tools (Read/Edit/Write) → filename
/// - Bash → command
/// - Search tools (Glob/Grep) → pattern
/// - Agent → description
/// - Task tools → task ID + subject/status
pub fn extract_tool_label(tool_name: &str, tool_input: &Value, tool_response: &Value) -> String {
    match tool_name {
        "Read" | "Edit" | "Write" | "NotebookEdit" => {
            let key = if tool_name == "NotebookEdit" {
                "notebook_path"
            } else {
                "file_path"
            };
            json_str(tool_input, key)
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string()
        }

        "Bash" => json_str(tool_input, "command").to_string(),

        "Glob" | "Grep" => json_str(tool_input, "pattern").to_string(),

        "Agent" => json_str(tool_input, "description").to_string(),

        "WebFetch" => {
            let url = json_str(tool_input, "url");
            url.strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(url)
                .to_string()
        }

        "WebSearch" => json_str(tool_input, "query").to_string(),

        "Skill" => json_str(tool_input, "skill").to_string(),

        "ToolSearch" => json_str(tool_input, "query").to_string(),

        "TaskCreate" => {
            let subject = json_str(tool_input, "subject");
            // Try to get the created task ID from the response
            let id = json_str(tool_response, "taskId");
            if !id.is_empty() {
                format!("#{id} {subject}")
            } else {
                subject.to_string()
            }
        }

        "TaskUpdate" => {
            let id = json_str(tool_input, "taskId");
            let status = json_str(tool_input, "status");
            if !status.is_empty() {
                format!("{status} #{id}")
            } else {
                format!("#{id}")
            }
        }

        "TaskGet" | "TaskStop" | "TaskOutput" => {
            let id = json_str(tool_input, "taskId");
            format!("#{id}")
        }

        "SendMessage" => json_str(tool_input, "to").to_string(),

        "TeamCreate" => json_str(tool_input, "team_name").to_string(),

        "LSP" => json_str(tool_input, "operation").to_string(),

        "AskUserQuestion" => {
            // Extract first question text
            if let Some(questions) = tool_input.get("questions").and_then(|q| q.as_array()) {
                if let Some(first) = questions.first() {
                    return json_str(first, "question").to_string();
                }
            }
            String::new()
        }

        "CronCreate" => json_str(tool_input, "cron").to_string(),

        "CronDelete" => json_str(tool_input, "id").to_string(),

        "EnterWorktree" => json_str(tool_input, "name").to_string(),

        _ => String::new(),
    }
}

fn json_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_read_label() {
        let input = json!({"file_path": "/home/user/project/src/main.rs"});
        assert_eq!(
            extract_tool_label("Read", &input, &Value::Null),
            "main.rs"
        );
    }

    #[test]
    fn test_bash_label() {
        let input = json!({"command": "cargo test"});
        assert_eq!(
            extract_tool_label("Bash", &input, &Value::Null),
            "cargo test"
        );
    }

    #[test]
    fn test_grep_label() {
        let input = json!({"pattern": "fn main"});
        assert_eq!(
            extract_tool_label("Grep", &input, &Value::Null),
            "fn main"
        );
    }

    #[test]
    fn test_agent_label() {
        let input = json!({"description": "Search codebase"});
        assert_eq!(
            extract_tool_label("Agent", &input, &Value::Null),
            "Search codebase"
        );
    }

    #[test]
    fn test_web_fetch_strips_https() {
        let input = json!({"url": "https://docs.rs/ratatui"});
        assert_eq!(
            extract_tool_label("WebFetch", &input, &Value::Null),
            "docs.rs/ratatui"
        );
    }

    #[test]
    fn test_task_create_with_id() {
        let input = json!({"subject": "Fix bug"});
        let response = json!({"taskId": "42"});
        assert_eq!(
            extract_tool_label("TaskCreate", &input, &response),
            "#42 Fix bug"
        );
    }

    #[test]
    fn test_task_update_label() {
        let input = json!({"taskId": "3", "status": "completed"});
        assert_eq!(
            extract_tool_label("TaskUpdate", &input, &Value::Null),
            "completed #3"
        );
    }

    #[test]
    fn test_ask_user_question_label() {
        let input = json!({
            "questions": [{"question": "Which approach?"}]
        });
        assert_eq!(
            extract_tool_label("AskUserQuestion", &input, &Value::Null),
            "Which approach?"
        );
    }

    #[test]
    fn test_unknown_tool() {
        let input = json!({});
        assert_eq!(extract_tool_label("UnknownTool", &input, &Value::Null), "");
    }

    #[test]
    fn test_notebook_edit_label() {
        let input = json!({"notebook_path": "/home/user/analysis.ipynb"});
        assert_eq!(
            extract_tool_label("NotebookEdit", &input, &Value::Null),
            "analysis.ipynb"
        );
    }

    #[test]
    fn test_write_label() {
        let input = json!({"file_path": "/tmp/output.txt"});
        assert_eq!(
            extract_tool_label("Write", &input, &Value::Null),
            "output.txt"
        );
    }

    #[test]
    fn test_web_search_label() {
        let input = json!({"query": "rust async await"});
        assert_eq!(
            extract_tool_label("WebSearch", &input, &Value::Null),
            "rust async await"
        );
    }

    #[test]
    fn test_web_fetch_strips_http() {
        let input = json!({"url": "http://example.com/page"});
        assert_eq!(
            extract_tool_label("WebFetch", &input, &Value::Null),
            "example.com/page"
        );
    }

    #[test]
    fn test_skill_label() {
        let input = json!({"skill": "commit"});
        assert_eq!(
            extract_tool_label("Skill", &input, &Value::Null),
            "commit"
        );
    }

    #[test]
    fn test_tool_search_label() {
        let input = json!({"query": "select:Read,Edit"});
        assert_eq!(
            extract_tool_label("ToolSearch", &input, &Value::Null),
            "select:Read,Edit"
        );
    }

    #[test]
    fn test_task_create_without_response_id() {
        let input = json!({"subject": "Do something"});
        let response = json!({});
        assert_eq!(
            extract_tool_label("TaskCreate", &input, &response),
            "Do something"
        );
    }

    #[test]
    fn test_task_get_label() {
        let input = json!({"taskId": "7"});
        assert_eq!(
            extract_tool_label("TaskGet", &input, &Value::Null),
            "#7"
        );
    }

    #[test]
    fn test_send_message_label() {
        let input = json!({"to": "agent-123"});
        assert_eq!(
            extract_tool_label("SendMessage", &input, &Value::Null),
            "agent-123"
        );
    }

    #[test]
    fn test_ask_user_question_empty_questions() {
        let input = json!({"questions": []});
        assert_eq!(
            extract_tool_label("AskUserQuestion", &input, &Value::Null),
            ""
        );
    }

    #[test]
    fn test_glob_label() {
        let input = json!({"pattern": "**/*.rs"});
        assert_eq!(
            extract_tool_label("Glob", &input, &Value::Null),
            "**/*.rs"
        );
    }
}
