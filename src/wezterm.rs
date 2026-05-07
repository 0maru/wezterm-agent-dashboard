use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::process::Command;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneStatus {
    Running,
    Waiting,
    Idle,
    Error,
    Unknown,
}

impl PaneStatus {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "idle" => Self::Idle,
            "error" => Self::Error,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Idle => "idle",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Running => "●",
            Self::Waiting => "◐",
            Self::Idle => "○",
            Self::Error => "✕",
            Self::Unknown => "·",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentType {
    Claude,
    Codex,
}

impl AgentType {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Default,
    Plan,
    AcceptEdits,
    Auto,
    BypassPermissions,
}

impl PermissionMode {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "plan" => Self::Plan,
            "acceptEdits" => Self::AcceptEdits,
            "auto" => Self::Auto,
            "bypassPermissions" => Self::BypassPermissions,
            _ => Self::Default,
        }
    }

    pub fn badge(&self) -> Option<&'static str> {
        match self {
            Self::Plan => Some("plan"),
            Self::AcceptEdits => Some("edit"),
            Self::Auto => Some("auto"),
            Self::BypassPermissions => Some("!"),
            Self::Default => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Pane info (parsed from wezterm cli list output)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub pane_id: u64,
    pub tab_id: u64,
    pub window_id: u64,
    pub workspace: String,
    pub pane_active: bool,
    pub status: PaneStatus,
    pub attention: bool,
    pub agent: AgentType,
    pub path: String,
    pub prompt: String,
    pub prompt_is_response: bool,
    pub started_at: Option<u64>,
    pub session_started_at: Option<u64>,
    pub turn_started_at: Option<u64>,
    pub wait_reason: String,
    pub permission_mode: PermissionMode,
    pub subagents: Vec<String>,
}

// ---------------------------------------------------------------------------
// Hierarchy: Workspace → Tab → Pane (analogous to tmux Session → Window → Pane)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TabInfo {
    pub tab_id: u64,
    pub tab_title: String,
    pub tab_active: bool,
    pub panes: Vec<PaneInfo>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub workspace_name: String,
    pub tabs: Vec<TabInfo>,
}

// ---------------------------------------------------------------------------
// Raw JSON structure from `wezterm cli list --format json`
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RawWezTermPane {
    pub window_id: u64,
    pub tab_id: u64,
    #[serde(default)]
    pub tab_title: String,
    pub pane_id: u64,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_zoomed: bool,
    #[serde(default)]
    pub tty_name: String,
    #[serde(default)]
    pub user_vars: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Query functions
// ---------------------------------------------------------------------------

/// Query all panes from WezTerm via `wezterm cli list --format json`.
pub fn query_all_panes() -> Vec<RawWezTermPane> {
    let output = Command::new("wezterm")
        .args(["cli", "list", "--format", "json"])
        .output()
        .ok();

    let output = match output {
        Some(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    serde_json::from_slice(&output.stdout).unwrap_or_default()
}

/// Build the workspace→tab→pane hierarchy from raw panes.
/// Only includes panes that have an `agent_type` user variable set.
/// Excludes the dashboard pane itself.
pub fn query_workspaces(dashboard_pane_id: Option<u64>) -> Vec<WorkspaceInfo> {
    let raw_panes = query_all_panes();
    build_workspaces(raw_panes, dashboard_pane_id)
}

/// Build hierarchy from raw panes (testable without subprocess).
pub fn build_workspaces(
    raw_panes: Vec<RawWezTermPane>,
    dashboard_pane_id: Option<u64>,
) -> Vec<WorkspaceInfo> {
    // workspace_name → (tab_id → TabInfo)
    let mut workspaces: IndexMap<String, IndexMap<u64, TabInfo>> = IndexMap::new();

    for raw in raw_panes {
        // Skip the dashboard pane itself
        if Some(raw.pane_id) == dashboard_pane_id {
            continue;
        }

        // Skip non-agent panes (pane_role = "dashboard" or no agent_type)
        if raw
            .user_vars
            .get("pane_role")
            .is_some_and(|r| r == "dashboard")
        {
            continue;
        }

        let pane_info = match parse_pane_info(&raw) {
            Some(p) => p,
            None => continue,
        };

        let workspace = workspaces.entry(raw.workspace.clone()).or_default();

        let tab = workspace.entry(raw.tab_id).or_insert_with(|| TabInfo {
            tab_id: raw.tab_id,
            tab_title: raw.tab_title.clone(),
            tab_active: false,
            panes: Vec::new(),
        });

        if raw.is_active {
            tab.tab_active = true;
        }

        tab.panes.push(pane_info);
    }

    workspaces
        .into_iter()
        .map(|(name, tabs)| WorkspaceInfo {
            workspace_name: name,
            tabs: tabs.into_values().collect(),
        })
        .collect()
}

/// Parse a raw WezTerm pane into a PaneInfo.
/// Returns None if the pane has no agent_type user variable.
fn parse_pane_info(raw: &RawWezTermPane) -> Option<PaneInfo> {
    let agent_type_str = raw.user_vars.get("agent_type")?;
    let agent = AgentType::from_str(agent_type_str)?;

    let status = raw
        .user_vars
        .get("agent_status")
        .map(|s| PaneStatus::from_str(s))
        .unwrap_or(PaneStatus::Unknown);

    let attention = raw
        .user_vars
        .get("agent_attention")
        .is_some_and(|s| !s.is_empty());

    // Resolve CWD: prefer agent_cwd user var, fallback to pane cwd
    let pane_cwd = raw.cwd.strip_prefix("file://").unwrap_or(&raw.cwd);
    let agent_cwd = raw.user_vars.get("agent_cwd").cloned().unwrap_or_default();
    let path = if !agent_cwd.is_empty() {
        agent_cwd
    } else {
        url_decode(pane_cwd)
    };

    let prompt = raw
        .user_vars
        .get("agent_prompt")
        .cloned()
        .unwrap_or_default();

    let prompt_is_response = raw
        .user_vars
        .get("agent_prompt_source")
        .is_some_and(|s| s == "response");

    let started_at = parse_time_var(&raw.user_vars, "agent_started_at");
    let session_started_at = parse_time_var(&raw.user_vars, "agent_session_started_at");
    let turn_started_at = parse_time_var(&raw.user_vars, "agent_turn_started_at").or(started_at);

    let wait_reason = raw
        .user_vars
        .get("agent_wait_reason")
        .cloned()
        .unwrap_or_default();

    let permission_mode = raw
        .user_vars
        .get("agent_permission_mode")
        .map(|s| PermissionMode::from_str(s))
        .unwrap_or(PermissionMode::Default);

    let subagents: Vec<String> = raw
        .user_vars
        .get("agent_subagents")
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|t| t.to_string()).collect())
        .unwrap_or_default();

    Some(PaneInfo {
        pane_id: raw.pane_id,
        tab_id: raw.tab_id,
        window_id: raw.window_id,
        workspace: raw.workspace.clone(),
        pane_active: raw.is_active,
        status,
        attention,
        agent,
        path,
        prompt,
        prompt_is_response,
        started_at: turn_started_at,
        session_started_at,
        turn_started_at,
        wait_reason,
        permission_mode,
        subagents,
    })
}

fn parse_time_var(user_vars: &HashMap<String, String>, key: &str) -> Option<u64> {
    user_vars.get(key).and_then(|s| s.parse::<u64>().ok())
}

/// Simple URL decoding for file:// paths (handles %20, etc.)
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h = chars.next().unwrap_or(b'0');
            let l = chars.next().unwrap_or(b'0');
            let byte = hex_byte(h) * 16 + hex_byte(l);
            result.push(byte as char);
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_byte(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Pane navigation
// ---------------------------------------------------------------------------

/// Activate (focus) a specific pane by ID.
pub fn activate_pane(pane_id: u64) {
    let _ = Command::new("wezterm")
        .args(["cli", "activate-pane", "--pane-id", &pane_id.to_string()])
        .output();
}

/// Activate a specific tab by ID.
pub fn activate_tab(tab_id: u64) {
    let _ = Command::new("wezterm")
        .args(["cli", "activate-tab", "--tab-id", &tab_id.to_string()])
        .output();
}

/// Jump to a pane: first activate its tab, then activate the pane.
pub fn jump_to_pane(tab_id: u64, pane_id: u64) {
    activate_tab(tab_id);
    activate_pane(pane_id);
}

/// Split a new pane to the right and run a command in it.
/// Returns the new pane ID if successful.
pub fn split_pane_right(percent: u8, args: &[&str]) -> Option<u64> {
    let mut cmd = Command::new("wezterm");
    cmd.args([
        "cli",
        "split-pane",
        "--right",
        "--percent",
        &percent.to_string(),
    ]);
    if !args.is_empty() {
        cmd.arg("--");
        cmd.args(args);
    }

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse::<u64>().ok()
}

/// Kill a pane by ID.
pub fn kill_pane(pane_id: u64) {
    let _ = Command::new("wezterm")
        .args(["cli", "kill-pane", "--pane-id", &pane_id.to_string()])
        .output();
}

/// Find the currently focused pane's ID (the active pane in the active tab).
pub fn find_focused_pane(workspaces: &[WorkspaceInfo], dashboard_pane_id: u64) -> Option<u64> {
    for ws in workspaces {
        for tab in &ws.tabs {
            for pane in &tab.panes {
                if pane.pane_active && pane.pane_id != dashboard_pane_id {
                    return Some(pane.pane_id);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_status_roundtrip() {
        for s in &["running", "waiting", "idle", "error", "unknown"] {
            let status = PaneStatus::from_str(s);
            assert_eq!(PaneStatus::from_str(status.as_str()), status);
        }
    }

    #[test]
    fn test_agent_type_from_str() {
        assert_eq!(AgentType::from_str("claude"), Some(AgentType::Claude));
        assert_eq!(AgentType::from_str("codex"), Some(AgentType::Codex));
        assert_eq!(AgentType::from_str("unknown"), None);
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("/Users/test%20dir/file"), "/Users/test dir/file");
        assert_eq!(url_decode("/simple/path"), "/simple/path");
    }

    #[test]
    fn test_permission_mode_badge() {
        assert_eq!(PermissionMode::Plan.badge(), Some("plan"));
        assert_eq!(PermissionMode::Auto.badge(), Some("auto"));
        assert_eq!(PermissionMode::Default.badge(), None);
    }

    #[test]
    fn test_build_workspaces_empty() {
        let result = build_workspaces(Vec::new(), None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_workspaces_filters_non_agent_panes() {
        let raw = vec![RawWezTermPane {
            window_id: 0,
            tab_id: 0,
            tab_title: "test".to_string(),
            pane_id: 1,
            workspace: "default".to_string(),
            title: "bash".to_string(),
            cwd: "file:///home/user".to_string(),
            is_active: true,
            is_zoomed: false,
            tty_name: String::new(),
            user_vars: HashMap::new(), // no agent_type
        }];

        let result = build_workspaces(raw, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_workspaces_includes_agent_panes() {
        let mut user_vars = HashMap::new();
        user_vars.insert("agent_type".to_string(), "claude".to_string());
        user_vars.insert("agent_status".to_string(), "running".to_string());

        let raw = vec![RawWezTermPane {
            window_id: 0,
            tab_id: 0,
            tab_title: "test".to_string(),
            pane_id: 1,
            workspace: "default".to_string(),
            title: "claude".to_string(),
            cwd: "file:///home/user/project".to_string(),
            is_active: true,
            is_zoomed: false,
            tty_name: String::new(),
            user_vars,
        }];

        let result = build_workspaces(raw, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tabs.len(), 1);
        assert_eq!(result[0].tabs[0].panes.len(), 1);
        assert_eq!(result[0].tabs[0].panes[0].agent, AgentType::Claude);
        assert_eq!(result[0].tabs[0].panes[0].status, PaneStatus::Running);
    }

    #[test]
    fn test_build_workspaces_parses_session_and_turn_times() {
        let mut user_vars = HashMap::new();
        user_vars.insert("agent_type".to_string(), "claude".to_string());
        user_vars.insert("agent_status".to_string(), "running".to_string());
        user_vars.insert("agent_started_at".to_string(), "90".to_string());
        user_vars.insert("agent_session_started_at".to_string(), "100".to_string());
        user_vars.insert("agent_turn_started_at".to_string(), "120".to_string());

        let raw = vec![RawWezTermPane {
            window_id: 0,
            tab_id: 0,
            tab_title: "test".to_string(),
            pane_id: 1,
            workspace: "default".to_string(),
            title: "claude".to_string(),
            cwd: "file:///home/user/project".to_string(),
            is_active: true,
            is_zoomed: false,
            tty_name: String::new(),
            user_vars,
        }];

        let result = build_workspaces(raw, None);
        let pane = &result[0].tabs[0].panes[0];

        assert_eq!(pane.started_at, Some(120));
        assert_eq!(pane.session_started_at, Some(100));
        assert_eq!(pane.turn_started_at, Some(120));
    }

    #[test]
    fn test_build_workspaces_uses_legacy_started_at_for_turn() {
        let mut user_vars = HashMap::new();
        user_vars.insert("agent_type".to_string(), "claude".to_string());
        user_vars.insert("agent_status".to_string(), "running".to_string());
        user_vars.insert("agent_started_at".to_string(), "90".to_string());

        let raw = vec![RawWezTermPane {
            window_id: 0,
            tab_id: 0,
            tab_title: "test".to_string(),
            pane_id: 1,
            workspace: "default".to_string(),
            title: "claude".to_string(),
            cwd: "file:///home/user/project".to_string(),
            is_active: true,
            is_zoomed: false,
            tty_name: String::new(),
            user_vars,
        }];

        let result = build_workspaces(raw, None);
        let pane = &result[0].tabs[0].panes[0];

        assert_eq!(pane.started_at, Some(90));
        assert_eq!(pane.session_started_at, None);
        assert_eq!(pane.turn_started_at, Some(90));
    }

    #[test]
    fn test_permission_mode_from_str() {
        assert_eq!(PermissionMode::from_str("plan"), PermissionMode::Plan);
        assert_eq!(
            PermissionMode::from_str("acceptEdits"),
            PermissionMode::AcceptEdits
        );
        assert_eq!(PermissionMode::from_str("auto"), PermissionMode::Auto);
        assert_eq!(
            PermissionMode::from_str("bypassPermissions"),
            PermissionMode::BypassPermissions
        );
        assert_eq!(PermissionMode::from_str("default"), PermissionMode::Default);
        assert_eq!(PermissionMode::from_str("unknown"), PermissionMode::Default);
        assert_eq!(PermissionMode::from_str(""), PermissionMode::Default);
    }

    #[test]
    fn test_pane_status_icons() {
        assert_eq!(PaneStatus::Running.icon(), "●");
        assert_eq!(PaneStatus::Waiting.icon(), "◐");
        assert_eq!(PaneStatus::Idle.icon(), "○");
        assert_eq!(PaneStatus::Error.icon(), "✕");
        assert_eq!(PaneStatus::Unknown.icon(), "·");
    }

    #[test]
    fn test_agent_type_display() {
        assert_eq!(format!("{}", AgentType::Claude), "claude");
        assert_eq!(format!("{}", AgentType::Codex), "codex");
    }

    #[test]
    fn test_find_focused_pane_returns_active() {
        let workspaces = vec![WorkspaceInfo {
            workspace_name: "default".into(),
            tabs: vec![TabInfo {
                tab_id: 0,
                tab_title: "test".into(),
                tab_active: true,
                panes: vec![
                    PaneInfo {
                        pane_id: 1,
                        tab_id: 0,
                        window_id: 0,
                        workspace: "default".into(),
                        pane_active: false,
                        status: PaneStatus::Idle,
                        attention: false,
                        agent: AgentType::Claude,
                        path: "/tmp".into(),
                        prompt: String::new(),
                        prompt_is_response: false,
                        started_at: None,
                        session_started_at: None,
                        turn_started_at: None,
                        wait_reason: String::new(),
                        permission_mode: PermissionMode::Default,
                        subagents: Vec::new(),
                    },
                    PaneInfo {
                        pane_id: 2,
                        tab_id: 0,
                        window_id: 0,
                        workspace: "default".into(),
                        pane_active: true,
                        status: PaneStatus::Running,
                        attention: false,
                        agent: AgentType::Claude,
                        path: "/tmp".into(),
                        prompt: String::new(),
                        prompt_is_response: false,
                        started_at: None,
                        session_started_at: None,
                        turn_started_at: None,
                        wait_reason: String::new(),
                        permission_mode: PermissionMode::Default,
                        subagents: Vec::new(),
                    },
                ],
            }],
        }];
        assert_eq!(find_focused_pane(&workspaces, 999), Some(2));
    }

    #[test]
    fn test_find_focused_pane_excludes_dashboard() {
        let workspaces = vec![WorkspaceInfo {
            workspace_name: "default".into(),
            tabs: vec![TabInfo {
                tab_id: 0,
                tab_title: "test".into(),
                tab_active: true,
                panes: vec![PaneInfo {
                    pane_id: 42,
                    tab_id: 0,
                    window_id: 0,
                    workspace: "default".into(),
                    pane_active: true,
                    status: PaneStatus::Running,
                    attention: false,
                    agent: AgentType::Claude,
                    path: "/tmp".into(),
                    prompt: String::new(),
                    prompt_is_response: false,
                    started_at: None,
                    session_started_at: None,
                    turn_started_at: None,
                    wait_reason: String::new(),
                    permission_mode: PermissionMode::Default,
                    subagents: Vec::new(),
                }],
            }],
        }];
        // pane 42 is the dashboard, should be excluded
        assert_eq!(find_focused_pane(&workspaces, 42), None);
    }

    #[test]
    fn test_find_focused_pane_empty() {
        assert_eq!(find_focused_pane(&[], 0), None);
    }

    #[test]
    fn test_build_workspaces_multiple_tabs() {
        let mut vars1 = HashMap::new();
        vars1.insert("agent_type".to_string(), "claude".to_string());
        vars1.insert("agent_status".to_string(), "running".to_string());

        let mut vars2 = HashMap::new();
        vars2.insert("agent_type".to_string(), "codex".to_string());
        vars2.insert("agent_status".to_string(), "idle".to_string());

        let raw = vec![
            RawWezTermPane {
                window_id: 0,
                tab_id: 1,
                tab_title: "tab1".to_string(),
                pane_id: 10,
                workspace: "ws".to_string(),
                title: "".to_string(),
                cwd: "file:///home/user".to_string(),
                is_active: true,
                is_zoomed: false,
                tty_name: String::new(),
                user_vars: vars1,
            },
            RawWezTermPane {
                window_id: 0,
                tab_id: 2,
                tab_title: "tab2".to_string(),
                pane_id: 20,
                workspace: "ws".to_string(),
                title: "".to_string(),
                cwd: "file:///home/user".to_string(),
                is_active: false,
                is_zoomed: false,
                tty_name: String::new(),
                user_vars: vars2,
            },
        ];

        let result = build_workspaces(raw, None);
        assert_eq!(result.len(), 1); // 1 workspace
        assert_eq!(result[0].tabs.len(), 2); // 2 tabs
        assert_eq!(result[0].tabs[0].panes[0].agent, AgentType::Claude);
        assert_eq!(result[0].tabs[1].panes[0].agent, AgentType::Codex);
    }

    #[test]
    fn test_build_workspaces_excludes_dashboard() {
        let mut user_vars = HashMap::new();
        user_vars.insert("agent_type".to_string(), "claude".to_string());
        user_vars.insert("agent_status".to_string(), "idle".to_string());

        let raw = vec![RawWezTermPane {
            window_id: 0,
            tab_id: 0,
            tab_title: "test".to_string(),
            pane_id: 42,
            workspace: "default".to_string(),
            title: "claude".to_string(),
            cwd: "file:///home/user".to_string(),
            is_active: false,
            is_zoomed: false,
            tty_name: String::new(),
            user_vars,
        }];

        // Exclude pane 42 (the dashboard)
        let result = build_workspaces(raw, Some(42));
        assert!(result.is_empty());
    }
}
