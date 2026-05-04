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
    let label = match tool_name {
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
            let id = first_json_str(tool_response, &["taskId", "task_id", "id"]);
            if !id.is_empty() {
                format!("#{id} {subject}")
            } else {
                subject.to_string()
            }
        }

        "TaskUpdate" => {
            let id = first_json_str(tool_input, &["taskId", "task_id", "id"]);
            let status = json_str(tool_input, "status");
            if !status.is_empty() {
                format!("{status} #{id}")
            } else {
                format!("#{id}")
            }
        }

        "TaskCreated" => {
            let subject = first_json_str(tool_input, &["subject", "title", "name"]);
            let mut id = first_json_str(tool_input, &["taskId", "task_id", "id"]);
            if id.is_empty() {
                id = first_json_str(tool_response, &["taskId", "task_id", "id"]);
            }
            if !id.is_empty() {
                format!("#{id} {subject}")
            } else {
                subject.to_string()
            }
        }

        "TaskCompleted" => {
            let mut id = first_json_str(tool_input, &["taskId", "task_id", "id"]);
            if id.is_empty() {
                id = first_json_str(tool_response, &["taskId", "task_id", "id"]);
            }
            format!("completed #{id}")
        }

        "TodoWrite" => todo_write_label(tool_input),

        "TaskGet" | "TaskStop" | "TaskOutput" => {
            let id = first_json_str(tool_input, &["taskId", "task_id", "id"]);
            format!("#{id}")
        }

        "SendMessage" => json_str(tool_input, "to").to_string(),

        "TeamCreate" => json_str(tool_input, "team_name").to_string(),

        "LSP" => json_str(tool_input, "operation").to_string(),

        "AskUserQuestion" => {
            // Extract first question text
            if let Some(questions) = tool_input.get("questions").and_then(|q| q.as_array())
                && let Some(first) = questions.first()
            {
                return json_str(first, "question").to_string();
            }
            String::new()
        }

        "CronCreate" => json_str(tool_input, "cron").to_string(),

        "CronDelete" => json_str(tool_input, "id").to_string(),

        "EnterWorktree" => json_str(tool_input, "name").to_string(),

        _ => String::new(),
    };

    append_error_label(label, tool_response)
}

fn json_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn first_json_str<'a>(value: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_str))
        .find(|value| !value.is_empty())
        .unwrap_or("")
}

fn todo_write_label(tool_input: &Value) -> String {
    let Some(todos) = tool_input.get("todos").and_then(Value::as_array) else {
        return String::new();
    };

    todos
        .iter()
        .enumerate()
        .filter_map(|(idx, todo)| {
            let status = json_str(todo, "status");
            if status.is_empty() {
                None
            } else {
                Some(format!("{status} #{}", idx + 1))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn append_error_label(label: String, tool_response: &Value) -> String {
    let Some(error) = response_error(tool_response) else {
        return label;
    };

    if label.is_empty() {
        format!("error: {error}")
    } else {
        format!("{label} error: {error}")
    }
}

fn response_error(value: &Value) -> Option<&str> {
    if let Some(error) = value.get("error") {
        if let Some(error) = error.as_str().filter(|error| !error.is_empty()) {
            return Some(error);
        }
        if let Some(error) = error.get("message").and_then(Value::as_str)
            && !error.is_empty()
        {
            return Some(error);
        }
    }

    if value.get("success").and_then(Value::as_bool) == Some(false) {
        let message = first_json_str(value, &["message", "stderr"]);
        if !message.is_empty() {
            return Some(message);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_read_label() {
        let input = json!({"file_path": "/home/user/project/src/main.rs"});
        assert_eq!(extract_tool_label("Read", &input, &Value::Null), "main.rs");
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
        assert_eq!(extract_tool_label("Grep", &input, &Value::Null), "fn main");
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
    fn test_task_created_label() {
        let input = json!({"id": "9", "title": "Map files"});
        assert_eq!(
            extract_tool_label("TaskCreated", &input, &Value::Null),
            "#9 Map files"
        );
    }

    #[test]
    fn test_task_completed_label() {
        let input = json!({"task_id": "9"});
        assert_eq!(
            extract_tool_label("TaskCompleted", &input, &Value::Null),
            "completed #9"
        );
    }

    #[test]
    fn test_todo_write_label() {
        let input = json!({
            "todos": [
                {"content": "Inspect", "status": "completed"},
                {"content": "Patch", "status": "in_progress"},
                {"content": "Verify", "status": "pending"}
            ]
        });

        assert_eq!(
            extract_tool_label("TodoWrite", &input, &Value::Null),
            "completed #1, in_progress #2, pending #3"
        );
    }

    #[test]
    fn test_failed_tool_label_keeps_error() {
        let response = json!({"success": false, "message": "permission denied"});

        assert_eq!(
            extract_tool_label("UnknownTool", &json!({}), &response),
            "error: permission denied"
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
        assert_eq!(extract_tool_label("Skill", &input, &Value::Null), "commit");
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
        assert_eq!(extract_tool_label("TaskGet", &input, &Value::Null), "#7");
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
        assert_eq!(extract_tool_label("Glob", &input, &Value::Null), "**/*.rs");
    }
}
