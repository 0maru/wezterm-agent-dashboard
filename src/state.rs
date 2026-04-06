use crate::activity::{self, ActivityEntry, TaskProgress};
use crate::git::GitData;
use crate::group::{self, RepoGroup};
use crate::ui::colors::ColorTheme;
use crate::wezterm::{self, AgentType, PaneInfo, PaneStatus, WorkspaceInfo};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Filter,
    Agents,
    ActivityLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentFilter {
    All,
    Running,
    Waiting,
    Idle,
    Error,
}

impl AgentFilter {
    pub fn next(&self) -> Self {
        match self {
            Self::All => Self::Running,
            Self::Running => Self::Waiting,
            Self::Waiting => Self::Idle,
            Self::Idle => Self::Error,
            Self::Error => Self::All,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::All => Self::Error,
            Self::Running => Self::All,
            Self::Waiting => Self::Running,
            Self::Idle => Self::Waiting,
            Self::Error => Self::Idle,
        }
    }

    pub fn matches(&self, status: PaneStatus) -> bool {
        match self {
            Self::All => true,
            Self::Running => status == PaneStatus::Running,
            Self::Waiting => status == PaneStatus::Waiting,
            Self::Idle => status == PaneStatus::Idle,
            Self::Error => status == PaneStatus::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoFilter {
    All,
    Repo(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Activity,
    GitStatus,
}

impl BottomTab {
    pub fn toggle(&self) -> Self {
        match self {
            Self::Activity => Self::GitStatus,
            Self::GitStatus => Self::Activity,
        }
    }
}

// ---------------------------------------------------------------------------
// Row target — maps a UI row to a pane
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RowTarget {
    pub pane_id: u64,
    pub tab_id: u64,
    pub agent: AgentType,
}

// ---------------------------------------------------------------------------
// Scroll state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    pub offset: usize,
    pub total_lines: usize,
    pub visible_height: usize,
}

impl ScrollState {
    pub fn scroll(&mut self, delta: isize) {
        let max = self.total_lines.saturating_sub(self.visible_height);
        if delta > 0 {
            self.offset = (self.offset + delta as usize).min(max);
        } else {
            self.offset = self.offset.saturating_sub((-delta) as usize);
        }
    }

    pub fn clamp(&mut self) {
        let max = self.total_lines.saturating_sub(self.visible_height);
        self.offset = self.offset.min(max);
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    pub now: u64,
    pub workspaces: Vec<WorkspaceInfo>,
    pub repo_groups: Vec<RepoGroup>,
    pub dashboard_pane_id: u64,
    pub sidebar_focused: bool,
    pub focus: Focus,
    pub spinner_frame: usize,

    // Agent list
    pub selected_agent_row: usize,
    pub agent_row_targets: Vec<RowTarget>,
    pub agents_scroll: ScrollState,
    pub line_to_row: Vec<Option<usize>>,

    // Activity log
    pub activity_entries: Vec<ActivityEntry>,
    pub activity_scroll: ScrollState,
    pub activity_max_entries: usize,

    // Focused pane tracking
    pub focused_pane_id: Option<u64>,
    pub prev_focused_pane_id: Option<u64>,

    // Theme
    pub theme: ColorTheme,

    // Bottom panel
    pub bottom_tab: BottomTab,
    pub git: GitData,
    pub git_scroll: ScrollState,

    // Task progress
    pub pane_task_progress: HashMap<u64, TaskProgress>,
    pub pane_task_dismissed: HashMap<u64, usize>,
    pub pane_inactive_since: HashMap<u64, u64>,

    // Agent tracking
    pub seen_agent_panes: HashSet<u64>,
    pub pane_tab_prefs: HashMap<u64, BottomTab>,

    // Filters
    pub agent_filter: AgentFilter,
    pub repo_filter: RepoFilter,
    pub repo_popup_open: bool,
    pub repo_popup_selected: usize,
    pub repo_popup_area: Option<ratatui::layout::Rect>,
    pub repo_button_col: u16,
}

impl AppState {
    pub fn new(dashboard_pane_id: u64) -> Self {
        Self {
            now: current_epoch(),
            workspaces: Vec::new(),
            repo_groups: Vec::new(),
            dashboard_pane_id,
            sidebar_focused: false,
            focus: Focus::Agents,
            spinner_frame: 0,
            selected_agent_row: 0,
            agent_row_targets: Vec::new(),
            agents_scroll: ScrollState::default(),
            line_to_row: Vec::new(),
            activity_entries: Vec::new(),
            activity_scroll: ScrollState::default(),
            activity_max_entries: 8,
            focused_pane_id: None,
            prev_focused_pane_id: None,
            theme: ColorTheme::default(),
            bottom_tab: BottomTab::Activity,
            git: GitData::default(),
            git_scroll: ScrollState::default(),
            pane_task_progress: HashMap::new(),
            pane_task_dismissed: HashMap::new(),
            pane_inactive_since: HashMap::new(),
            seen_agent_panes: HashSet::new(),
            pane_tab_prefs: HashMap::new(),
            agent_filter: AgentFilter::All,
            repo_filter: RepoFilter::All,
            repo_popup_open: false,
            repo_popup_selected: 0,
            repo_popup_area: None,
            repo_button_col: 0,
        }
    }

    /// Refresh state from WezTerm.
    pub fn refresh(&mut self) {
        self.now = current_epoch();

        // Query all workspaces
        self.workspaces = wezterm::query_workspaces(Some(self.dashboard_pane_id));

        // Find focused pane
        let new_focused = wezterm::find_focused_pane(&self.workspaces, self.dashboard_pane_id);
        if let Some(fid) = new_focused {
            if self.focused_pane_id != Some(fid) {
                self.prev_focused_pane_id = self.focused_pane_id;
                self.focused_pane_id = Some(fid);
            }
        }

        // Group panes by repo
        self.repo_groups = group::group_panes_by_repo(&self.workspaces, self.focused_pane_id);

        // Rebuild row targets
        self.rebuild_row_targets();

        // Refresh activity log for focused pane
        self.refresh_activity();

        // Refresh task progress
        self.refresh_task_progress();

        // Write focused pane path for git thread
        self.write_git_path();
    }

    /// Rebuild the flat list of selectable agent rows from groups.
    fn rebuild_row_targets(&mut self) {
        // Validate repo filter
        if let RepoFilter::Repo(ref name) = self.repo_filter {
            if !self.repo_groups.iter().any(|g| g.name == *name) {
                self.repo_filter = RepoFilter::All;
            }
        }

        self.agent_row_targets.clear();

        for group in &self.repo_groups {
            if let RepoFilter::Repo(ref name) = self.repo_filter {
                if group.name != *name {
                    continue;
                }
            }

            for (pane, _git_info) in &group.panes {
                if !self.agent_filter.matches(pane.status) {
                    continue;
                }

                self.agent_row_targets.push(RowTarget {
                    pane_id: pane.pane_id,
                    tab_id: pane.tab_id,
                    agent: pane.agent,
                });
            }
        }

        // Clamp selected row
        if !self.agent_row_targets.is_empty() {
            self.selected_agent_row = self
                .selected_agent_row
                .min(self.agent_row_targets.len() - 1);
        } else {
            self.selected_agent_row = 0;
        }
    }

    fn refresh_activity(&mut self) {
        if let Some(pane_id) = self.focused_pane_id {
            self.activity_entries = activity::read_activity_log(pane_id, self.activity_max_entries);
        } else if let Some(first) = self.agent_row_targets.first() {
            self.activity_entries =
                activity::read_activity_log(first.pane_id, self.activity_max_entries);
        } else {
            self.activity_entries.clear();
        }
    }

    fn refresh_task_progress(&mut self) {
        let mut active_panes = HashSet::new();

        for target in &self.agent_row_targets {
            active_panes.insert(target.pane_id);

            let entries = activity::read_activity_log(target.pane_id, 100);
            let progress = activity::parse_task_progress(&entries);

            if progress.is_empty() {
                self.pane_task_progress.remove(&target.pane_id);
            } else {
                self.pane_task_progress.insert(target.pane_id, progress);
            }
        }

        // Clean up stale entries
        self.pane_task_progress.retain(|id, _| active_panes.contains(id));
    }

    fn write_git_path(&self) {
        if let Some(pane_id) = self.focused_pane_id {
            // Find the focused pane's path
            for group in &self.repo_groups {
                for (pane, _) in &group.panes {
                    if pane.pane_id == pane_id {
                        let _ = std::fs::write(
                            "/tmp/wezterm-agent-dashboard-git-path",
                            &pane.path,
                        );
                        return;
                    }
                }
            }
        }
    }

    /// Count agents by status, respecting current repo filter.
    pub fn status_counts(&self) -> (usize, usize, usize, usize, usize) {
        let mut all = 0;
        let mut running = 0;
        let mut waiting = 0;
        let mut idle = 0;
        let mut error = 0;

        for group in &self.repo_groups {
            if let RepoFilter::Repo(ref name) = self.repo_filter {
                if group.name != *name {
                    continue;
                }
            }
            for (pane, _) in &group.panes {
                all += 1;
                match pane.status {
                    PaneStatus::Running => running += 1,
                    PaneStatus::Waiting => waiting += 1,
                    PaneStatus::Idle => idle += 1,
                    PaneStatus::Error => error += 1,
                    PaneStatus::Unknown => {}
                }
            }
        }

        (all, running, waiting, idle, error)
    }

    /// Get the currently selected pane info.
    pub fn selected_pane(&self) -> Option<&RowTarget> {
        self.agent_row_targets.get(self.selected_agent_row)
    }

    /// Navigate to the selected agent's pane.
    pub fn jump_to_selected(&self) {
        if let Some(target) = self.selected_pane() {
            wezterm::jump_to_pane(target.tab_id, target.pane_id);
        }
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected_agent_row > 0 {
            self.selected_agent_row -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if self.selected_agent_row + 1 < self.agent_row_targets.len() {
            self.selected_agent_row += 1;
        }
    }

    /// Get unique repo names for the repo filter popup.
    pub fn repo_names(&self) -> Vec<String> {
        self.repo_groups.iter().map(|g| g.name.clone()).collect()
    }

    /// Find a specific pane by ID across all groups.
    pub fn find_pane(&self, pane_id: u64) -> Option<&PaneInfo> {
        for group in &self.repo_groups {
            for (pane, _) in &group.panes {
                if pane.pane_id == pane_id {
                    return Some(pane);
                }
            }
        }
        None
    }
}

fn current_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
