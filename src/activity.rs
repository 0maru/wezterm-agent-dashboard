use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ActivityEntry {
    pub timestamp: String,
    pub tool: String,
    pub label: String,
}

impl ActivityEntry {
    /// Map tool names to 256-color indices for display.
    pub fn tool_color_index(&self) -> u8 {
        match self.tool.as_str() {
            "Edit" | "Write" => 180,         // soft yellow
            "Bash" => 114,                   // soft green
            "Read" | "Glob" | "Grep" => 110, // soft blue
            "Agent" => 181,                  // soft pink
            "WebFetch" | "WebSearch" => 117, // soft cyan
            "TaskCreate" | "TaskUpdate" | "TaskGet" | "TaskStop" | "TaskOutput" => 223, // soft gold
            "Skill" => 182,                  // light purple
            "AskUserQuestion" => 216,        // soft orange
            "SendMessage" | "TeamCreate" => 151, // muted green
            "LSP" => 146,                    // light lavender
            _ => 252,                        // light gray
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Default)]
pub struct TaskProgress {
    pub tasks: Vec<(String, TaskStatus)>,
}

impl TaskProgress {
    pub fn completed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|(_, s)| *s == TaskStatus::Completed)
            .count()
    }

    pub fn total_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn all_completed(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(|(_, s)| *s == TaskStatus::Completed)
    }

    pub fn display(&self) -> String {
        format!("{}/{}", self.completed_count(), self.total_count())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFingerprint {
    pub exists: bool,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl LogFingerprint {
    pub fn missing() -> Self {
        Self {
            exists: false,
            len: 0,
            modified: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ActivitySnapshot {
    pub display_entries: Vec<ActivityEntry>,
    pub task_progress: TaskProgress,
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

/// Path to the activity log file for a given pane ID.
pub fn log_file_path(pane_id: u64) -> PathBuf {
    PathBuf::from(format!("/tmp/wezterm-agent-activity-{pane_id}.log"))
}

/// Read activity log entries for a pane, returning newest-first, up to max_entries.
pub fn read_activity_log(pane_id: u64, max_entries: usize) -> Vec<ActivityEntry> {
    let path = log_file_path(pane_id);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .rev()
        .filter(|line| !line.is_empty())
        .take(max_entries)
        .filter_map(parse_activity_line)
        .collect()
}

pub fn log_fingerprint(pane_id: u64) -> LogFingerprint {
    let path = log_file_path(pane_id);
    match fs::metadata(path) {
        Ok(meta) => LogFingerprint {
            exists: true,
            len: meta.len(),
            modified: meta.modified().ok(),
        },
        Err(_) => LogFingerprint::missing(),
    }
}

pub fn read_activity_snapshot(
    pane_id: u64,
    display_limit: usize,
    progress_limit: usize,
) -> ActivitySnapshot {
    let entries = read_activity_log(pane_id, display_limit.max(progress_limit));
    let display_entries = entries.iter().take(display_limit).cloned().collect();
    let progress_entries = entries
        .iter()
        .take(progress_limit)
        .cloned()
        .collect::<Vec<_>>();
    let task_progress = parse_task_progress(&progress_entries);

    ActivitySnapshot {
        display_entries,
        task_progress,
    }
}

fn parse_activity_line(line: &str) -> Option<ActivityEntry> {
    let mut parts = line.splitn(3, '|');
    let timestamp = parts.next()?.to_string();
    let tool = parts.next()?.to_string();
    let label = parts.next().unwrap_or("").to_string();
    Some(ActivityEntry {
        timestamp,
        tool,
        label,
    })
}

// ---------------------------------------------------------------------------
// Task progress parsing
// ---------------------------------------------------------------------------

/// Parse task progress from activity log entries (in chronological order).
pub fn parse_task_progress(entries: &[ActivityEntry]) -> TaskProgress {
    let mut progress = TaskProgress::default();

    // Process entries in chronological order (they come newest-first, so reverse)
    for entry in entries.iter().rev() {
        match entry.tool.as_str() {
            "TaskCreate" => {
                let id = extract_task_id_from_create(&entry.label);
                if !id.is_empty() {
                    // Check if this ID already exists (new session reuse)
                    if progress.tasks.iter().any(|(tid, _)| tid == &id) {
                        // Reset — new batch
                        progress.tasks.clear();
                    }
                    // Also reset if all previous tasks are completed
                    if progress.all_completed() {
                        progress.tasks.clear();
                    }
                    progress.tasks.push((id, TaskStatus::Pending));
                }
            }
            "TaskUpdate" => {
                let (status_str, id) = extract_status_and_id(&entry.label);
                if !id.is_empty() {
                    let new_status = match status_str {
                        "completed" => TaskStatus::Completed,
                        "in_progress" => TaskStatus::InProgress,
                        "deleted" => {
                            progress.tasks.retain(|(tid, _)| tid != &id);
                            continue;
                        }
                        _ => continue,
                    };
                    if let Some(task) = progress.tasks.iter_mut().find(|(tid, _)| tid == &id) {
                        task.1 = new_status;
                    }
                }
            }
            _ => {}
        }
    }

    progress
}

fn extract_task_id_from_create(label: &str) -> String {
    // Format: "#42 Subject text" or just "Subject text"
    if label.starts_with('#') {
        label
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_start_matches('#')
            .to_string()
    } else {
        String::new()
    }
}

fn extract_status_and_id(label: &str) -> (&str, String) {
    // Format: "completed #42" or "in_progress #3"
    let parts: Vec<&str> = label.splitn(2, ' ').collect();
    if parts.len() == 2 {
        let status = parts[0];
        let id = parts[1].trim_start_matches('#').to_string();
        (status, id)
    } else {
        ("", String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_activity_line() {
        let entry = parse_activity_line("14:32|Edit|main.rs").unwrap();
        assert_eq!(entry.timestamp, "14:32");
        assert_eq!(entry.tool, "Edit");
        assert_eq!(entry.label, "main.rs");
    }

    #[test]
    fn test_parse_activity_line_empty_label() {
        let entry = parse_activity_line("14:32|Bash|").unwrap();
        assert_eq!(entry.label, "");
    }

    #[test]
    fn test_parse_activity_line_with_pipes_in_label() {
        let entry = parse_activity_line("14:32|Bash|echo hello world").unwrap();
        assert_eq!(entry.label, "echo hello world");
    }

    #[test]
    fn test_task_progress_display() {
        let progress = TaskProgress {
            tasks: vec![
                ("1".into(), TaskStatus::Completed),
                ("2".into(), TaskStatus::InProgress),
                ("3".into(), TaskStatus::Pending),
            ],
        };
        assert_eq!(progress.display(), "1/3");
        assert!(!progress.all_completed());
    }

    #[test]
    fn test_parse_task_progress() {
        let entries = vec![
            ActivityEntry {
                timestamp: "14:30".into(),
                tool: "TaskCreate".into(),
                label: "#1 First task".into(),
            },
            ActivityEntry {
                timestamp: "14:31".into(),
                tool: "TaskCreate".into(),
                label: "#2 Second task".into(),
            },
            ActivityEntry {
                timestamp: "14:32".into(),
                tool: "TaskUpdate".into(),
                label: "completed #1".into(),
            },
        ];

        // entries are newest-first, so reverse order
        let reversed: Vec<_> = entries.into_iter().rev().collect();
        let progress = parse_task_progress(&reversed);
        assert_eq!(progress.total_count(), 2);
        assert_eq!(progress.completed_count(), 1);
    }

    #[test]
    fn test_tool_color_index() {
        let entry = ActivityEntry {
            timestamp: "".into(),
            tool: "Bash".into(),
            label: "".into(),
        };
        assert_eq!(entry.tool_color_index(), 114);
    }

    #[test]
    fn test_tool_color_index_variants() {
        let cases = vec![
            ("Edit", 180),
            ("Write", 180),
            ("Read", 110),
            ("Glob", 110),
            ("Grep", 110),
            ("Agent", 181),
            ("WebFetch", 117),
            ("WebSearch", 117),
            ("TaskCreate", 223),
            ("TaskUpdate", 223),
            ("Skill", 182),
            ("AskUserQuestion", 216),
            ("UnknownTool", 252),
        ];
        for (tool, expected) in cases {
            let entry = ActivityEntry {
                timestamp: "".into(),
                tool: tool.into(),
                label: "".into(),
            };
            assert_eq!(entry.tool_color_index(), expected, "tool={tool}");
        }
    }

    #[test]
    fn test_task_progress_empty() {
        let progress = TaskProgress::default();
        assert!(progress.is_empty());
        assert_eq!(progress.total_count(), 0);
        assert_eq!(progress.completed_count(), 0);
        assert!(!progress.all_completed()); // empty is not "all completed"
        assert_eq!(progress.display(), "0/0");
    }

    #[test]
    fn test_task_progress_all_completed() {
        let progress = TaskProgress {
            tasks: vec![
                ("1".into(), TaskStatus::Completed),
                ("2".into(), TaskStatus::Completed),
            ],
        };
        assert!(progress.all_completed());
        assert_eq!(progress.display(), "2/2");
    }

    #[test]
    fn test_parse_task_progress_delete() {
        let entries = vec![
            ActivityEntry {
                timestamp: "14:32".into(),
                tool: "TaskUpdate".into(),
                label: "deleted #1".into(),
            },
            ActivityEntry {
                timestamp: "14:31".into(),
                tool: "TaskCreate".into(),
                label: "#1 Task one".into(),
            },
        ];
        // Entries are newest-first, parse_task_progress reverses them
        let progress = parse_task_progress(&entries);
        assert_eq!(progress.total_count(), 0); // deleted
    }

    #[test]
    fn test_parse_task_progress_status_update() {
        let entries = vec![
            ActivityEntry {
                timestamp: "14:33".into(),
                tool: "TaskUpdate".into(),
                label: "in_progress #2".into(),
            },
            ActivityEntry {
                timestamp: "14:32".into(),
                tool: "TaskCreate".into(),
                label: "#2 Second task".into(),
            },
            ActivityEntry {
                timestamp: "14:31".into(),
                tool: "TaskCreate".into(),
                label: "#1 First task".into(),
            },
        ];
        let progress = parse_task_progress(&entries);
        assert_eq!(progress.total_count(), 2);
        assert_eq!(progress.tasks[0].1, TaskStatus::Pending); // #1
        assert_eq!(progress.tasks[1].1, TaskStatus::InProgress); // #2
    }

    #[test]
    fn test_extract_task_id_from_create_no_hash() {
        assert_eq!(extract_task_id_from_create("Just a subject"), "");
    }

    #[test]
    fn test_extract_task_id_from_create_with_hash() {
        assert_eq!(extract_task_id_from_create("#42 Fix the bug"), "42");
    }

    #[test]
    fn test_extract_status_and_id() {
        let (status, id) = extract_status_and_id("completed #5");
        assert_eq!(status, "completed");
        assert_eq!(id, "5");
    }

    #[test]
    fn test_extract_status_and_id_no_space() {
        let (status, id) = extract_status_and_id("completed");
        assert_eq!(status, "");
        assert_eq!(id, "");
    }

    #[test]
    fn test_parse_activity_line_missing_fields() {
        assert!(parse_activity_line("only_one_field").is_none());
    }

    #[test]
    fn test_log_file_path() {
        let path = log_file_path(123);
        assert_eq!(
            path.to_str().unwrap(),
            "/tmp/wezterm-agent-activity-123.log"
        );
    }
}
