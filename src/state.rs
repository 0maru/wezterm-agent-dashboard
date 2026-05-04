use crate::activity::{self, ActivityEntry, ActivitySnapshot, LogFingerprint, TaskProgress};
use crate::git::GitData;
use crate::group::{self, PaneGitInfo, RepoGroup, RepoInfoUpdate};
use crate::ui::colors::ColorTheme;
use crate::wezterm::{self, AgentType, PaneInfo, PaneStatus, WorkspaceInfo};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const ACTIVITY_MAX_ENTRIES: usize = 8;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshActions {
    pub git_path: Option<String>,
    pub repo_paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    pub now: u64,
    pub workspaces: Vec<WorkspaceInfo>,
    pub repo_groups: Vec<RepoGroup>,
    pub dashboard_pane_id: u64,
    pub focus: Focus,
    pub spinner_frame: usize,

    // Agent list
    pub selected_agent_row: usize,
    pub agent_row_targets: Vec<RowTarget>,

    // Activity log
    pub activity_entries: Vec<ActivityEntry>,

    // Focused pane tracking
    pub focused_pane_id: Option<u64>,

    // Theme
    pub theme: ColorTheme,

    // Bottom panel
    pub bottom_tab: BottomTab,
    pub git: GitData,

    // Task progress
    pub pane_task_progress: HashMap<u64, TaskProgress>,
    pub activity_cache: HashMap<u64, CachedActivity>,
    pub repo_info_cache: HashMap<String, PaneGitInfo>,
    pub pending_repo_paths: HashSet<String>,
    pub last_git_path: Option<String>,

    // Filters
    pub agent_filter: AgentFilter,
    pub repo_filter: RepoFilter,
    pub repo_popup_open: bool,
    pub repo_popup_selected: usize,
}

#[derive(Debug, Clone)]
pub struct CachedActivity {
    pub fingerprint: LogFingerprint,
    pub snapshot: ActivitySnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivityCachePolicy {
    RefreshIfChanged,
    ReuseCached,
}

impl AppState {
    pub fn new(dashboard_pane_id: u64) -> Self {
        Self {
            now: current_epoch(),
            workspaces: Vec::new(),
            repo_groups: Vec::new(),
            dashboard_pane_id,
            focus: Focus::Agents,
            spinner_frame: 0,
            selected_agent_row: 0,
            agent_row_targets: Vec::new(),
            activity_entries: Vec::new(),
            focused_pane_id: None,
            theme: ColorTheme::default(),
            bottom_tab: BottomTab::Activity,
            git: GitData::default(),
            pane_task_progress: HashMap::new(),
            activity_cache: HashMap::new(),
            repo_info_cache: HashMap::new(),
            pending_repo_paths: HashSet::new(),
            last_git_path: None,
            agent_filter: AgentFilter::All,
            repo_filter: RepoFilter::All,
            repo_popup_open: false,
            repo_popup_selected: 0,
        }
    }

    /// Refresh state from WezTerm.
    pub fn refresh(&mut self) -> RefreshActions {
        self.now = current_epoch();

        self.refresh_wezterm_snapshot();
        let repo_paths = self.refresh_repo_groups();
        let git_path = self.refresh_local_views_with_policy(ActivityCachePolicy::RefreshIfChanged);

        RefreshActions {
            git_path,
            repo_paths,
        }
    }

    /// Refresh only derived UI state after local filter changes.
    pub fn refresh_local_views(&mut self) -> Option<String> {
        self.now = current_epoch();
        self.refresh_local_views_with_policy(ActivityCachePolicy::ReuseCached)
    }

    pub fn apply_repo_info_updates(&mut self, updates: Vec<RepoInfoUpdate>) -> RefreshActions {
        if updates.is_empty() {
            return RefreshActions::default();
        }

        let mut changed = false;
        for update in updates {
            changed |= self.store_repo_info(update.path, update.info);
        }

        if !changed {
            return RefreshActions::default();
        }

        let repo_paths = self.refresh_repo_groups();
        let git_path = self.refresh_local_views_with_policy(ActivityCachePolicy::ReuseCached);

        RefreshActions {
            git_path,
            repo_paths,
        }
    }

    fn refresh_wezterm_snapshot(&mut self) {
        self.workspaces = wezterm::query_workspaces(Some(self.dashboard_pane_id));

        let new_focused = wezterm::find_focused_pane(&self.workspaces, self.dashboard_pane_id);
        if let Some(fid) = new_focused
            && self.focused_pane_id != Some(fid)
        {
            self.focused_pane_id = Some(fid);
        }
    }

    fn refresh_repo_groups(&mut self) -> Vec<String> {
        self.repo_groups = group::group_panes_by_repo(
            &self.workspaces,
            self.focused_pane_id,
            &self.repo_info_cache,
        );
        self.prune_repo_cache();
        self.collect_missing_repo_paths()
    }

    fn prune_repo_cache(&mut self) {
        let mut active_paths = HashSet::new();
        for ws in &self.workspaces {
            for tab in &ws.tabs {
                for pane in &tab.panes {
                    active_paths.insert(pane.path.clone());
                }
            }
        }
        self.repo_info_cache.retain(|cache_key, info| {
            active_paths.contains(cache_key)
                || info.repo_root.as_ref().is_some_and(|repo_root| {
                    let repo_root = Path::new(repo_root);
                    active_paths
                        .iter()
                        .any(|path| Path::new(path).starts_with(repo_root))
                })
        });
        self.pending_repo_paths
            .retain(|path| active_paths.contains(path));
    }

    fn collect_missing_repo_paths(&mut self) -> Vec<String> {
        let mut missing_paths = Vec::new();

        for ws in &self.workspaces {
            for tab in &ws.tabs {
                for pane in &tab.panes {
                    if group::lookup_cached_git_info_for_path(&pane.path, &self.repo_info_cache)
                        .is_some()
                    {
                        continue;
                    }

                    if self.pending_repo_paths.insert(pane.path.clone()) {
                        missing_paths.push(pane.path.clone());
                    }
                }
            }
        }

        missing_paths
    }

    fn store_repo_info(&mut self, path: String, info: PaneGitInfo) -> bool {
        self.pending_repo_paths.remove(&path);

        let mut changed = self
            .repo_info_cache
            .insert(path.clone(), info.clone())
            .as_ref()
            != Some(&info);

        if let Some(repo_root) = info.repo_root.as_ref() {
            changed |= self
                .repo_info_cache
                .insert(repo_root.clone(), info.clone())
                .as_ref()
                != Some(&info);
        }

        changed
    }

    /// Rebuild the flat list of selectable agent rows from groups.
    pub(crate) fn rebuild_row_targets(&mut self) {
        // Validate repo filter
        if let RepoFilter::Repo(ref id) = self.repo_filter
            && !self.repo_groups.iter().any(|g| g.id == *id)
        {
            self.repo_filter = RepoFilter::All;
        }

        self.agent_row_targets.clear();

        for group in &self.repo_groups {
            if let RepoFilter::Repo(ref id) = self.repo_filter
                && group.id != *id
            {
                continue;
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

    fn refresh_local_views_with_policy(
        &mut self,
        activity_cache_policy: ActivityCachePolicy,
    ) -> Option<String> {
        self.rebuild_row_targets();
        self.refresh_activity_cache(activity_cache_policy);
        self.refresh_activity_view();
        self.refresh_git_target()
    }

    fn tracked_activity_panes(&self) -> HashSet<u64> {
        let mut tracked_panes: HashSet<u64> = self
            .agent_row_targets
            .iter()
            .map(|target| target.pane_id)
            .collect();

        if let Some(pane_id) = self.active_focus_pane_id() {
            tracked_panes.insert(pane_id);
        }

        tracked_panes
    }

    fn active_focus_pane_id(&self) -> Option<u64> {
        self.focused_pane_id
            .filter(|pane_id| self.find_pane(*pane_id).is_some())
    }

    fn refresh_activity_cache(&mut self, policy: ActivityCachePolicy) {
        let tracked_panes = self.tracked_activity_panes();

        for pane_id in &tracked_panes {
            let fingerprint = match policy {
                ActivityCachePolicy::RefreshIfChanged => Some(activity::log_fingerprint(*pane_id)),
                ActivityCachePolicy::ReuseCached => None,
            };

            let should_refresh = match (self.activity_cache.get(pane_id), fingerprint.as_ref()) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(cached), Some(fingerprint)) => cached.fingerprint != *fingerprint,
            };

            if should_refresh {
                let fingerprint =
                    fingerprint.unwrap_or_else(|| activity::log_fingerprint(*pane_id));
                let snapshot =
                    activity::read_activity_snapshot(*pane_id, ACTIVITY_MAX_ENTRIES, 100);
                self.activity_cache.insert(
                    *pane_id,
                    CachedActivity {
                        fingerprint,
                        snapshot,
                    },
                );
            }
        }

        self.activity_cache
            .retain(|id, _| tracked_panes.contains(id));
    }

    fn refresh_activity_view(&mut self) {
        let activity_pane_id = self
            .active_focus_pane_id()
            .or_else(|| self.agent_row_targets.first().map(|target| target.pane_id));

        self.activity_entries = activity_pane_id
            .and_then(|pane_id| self.activity_cache.get(&pane_id))
            .map(|cached| cached.snapshot.display_entries.clone())
            .unwrap_or_default();

        self.pane_task_progress.clear();
        for target in &self.agent_row_targets {
            let Some(cached) = self.activity_cache.get(&target.pane_id) else {
                continue;
            };

            if !cached.snapshot.task_progress.is_empty() {
                self.pane_task_progress
                    .insert(target.pane_id, cached.snapshot.task_progress.clone());
            }
        }
    }

    fn refresh_git_target(&mut self) -> Option<String> {
        let path = self.active_focus_pane_id().and_then(|pane_id| {
            self.repo_groups
                .iter()
                .flat_map(|group| group.panes.iter())
                .find(|(pane, _)| pane.pane_id == pane_id)
                .map(|(pane, git_info)| {
                    git_info
                        .repo_root
                        .clone()
                        .unwrap_or_else(|| pane.path.clone())
                })
        });

        if path != self.last_git_path {
            self.last_git_path = path.clone();
            return Some(path.unwrap_or_default());
        }

        None
    }

    /// Count agents by status, respecting current repo filter.
    pub fn status_counts(&self) -> (usize, usize, usize, usize, usize) {
        let mut all = 0;
        let mut running = 0;
        let mut waiting = 0;
        let mut idle = 0;
        let mut error = 0;

        for group in &self.repo_groups {
            if let RepoFilter::Repo(ref id) = self.repo_filter
                && group.id != *id
            {
                continue;
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

    /// Get repo entries for the filter popup: (id, display_name).
    pub fn repo_entries(&self) -> Vec<(String, String)> {
        self.repo_groups
            .iter()
            .map(|g| (g.id.clone(), g.name.clone()))
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{PaneGitInfo, RepoGroup};
    use crate::wezterm::{AgentType, PaneInfo, PaneStatus, PermissionMode};

    fn make_pane(pane_id: u64, status: PaneStatus) -> PaneInfo {
        PaneInfo {
            pane_id,
            tab_id: 0,
            window_id: 0,
            workspace: "default".into(),
            pane_active: false,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/tmp/test".into(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            session_started_at: None,
            turn_started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: Vec::new(),
        }
    }

    fn make_git_info() -> PaneGitInfo {
        PaneGitInfo {
            repo_root: Some("/tmp/test".into()),
            branch: Some("main".into()),
            is_worktree: false,
        }
    }

    // -- AgentFilter tests --

    #[test]
    fn test_agent_filter_next_cycle() {
        let mut f = AgentFilter::All;
        f = f.next();
        assert_eq!(f, AgentFilter::Running);
        f = f.next();
        assert_eq!(f, AgentFilter::Waiting);
        f = f.next();
        assert_eq!(f, AgentFilter::Idle);
        f = f.next();
        assert_eq!(f, AgentFilter::Error);
        f = f.next();
        assert_eq!(f, AgentFilter::All);
    }

    #[test]
    fn test_agent_filter_prev_cycle() {
        let mut f = AgentFilter::All;
        f = f.prev();
        assert_eq!(f, AgentFilter::Error);
        f = f.prev();
        assert_eq!(f, AgentFilter::Idle);
        f = f.prev();
        assert_eq!(f, AgentFilter::Waiting);
        f = f.prev();
        assert_eq!(f, AgentFilter::Running);
        f = f.prev();
        assert_eq!(f, AgentFilter::All);
    }

    #[test]
    fn test_agent_filter_matches() {
        assert!(AgentFilter::All.matches(PaneStatus::Running));
        assert!(AgentFilter::All.matches(PaneStatus::Idle));
        assert!(AgentFilter::Running.matches(PaneStatus::Running));
        assert!(!AgentFilter::Running.matches(PaneStatus::Idle));
        assert!(AgentFilter::Waiting.matches(PaneStatus::Waiting));
        assert!(!AgentFilter::Waiting.matches(PaneStatus::Error));
        assert!(AgentFilter::Idle.matches(PaneStatus::Idle));
        assert!(!AgentFilter::Idle.matches(PaneStatus::Running));
        assert!(AgentFilter::Error.matches(PaneStatus::Error));
        assert!(!AgentFilter::Error.matches(PaneStatus::Waiting));
    }

    // -- BottomTab tests --

    #[test]
    fn test_bottom_tab_toggle() {
        assert_eq!(BottomTab::Activity.toggle(), BottomTab::GitStatus);
        assert_eq!(BottomTab::GitStatus.toggle(), BottomTab::Activity);
    }

    // -- AppState tests --

    fn make_state_with_groups(groups: Vec<RepoGroup>) -> AppState {
        let mut state = AppState::new(999);
        state.repo_groups = groups;
        // Manually build row targets
        state.agent_row_targets.clear();
        for group in &state.repo_groups {
            for (pane, _) in &group.panes {
                if state.agent_filter.matches(pane.status) {
                    state.agent_row_targets.push(RowTarget {
                        pane_id: pane.pane_id,
                        tab_id: pane.tab_id,
                        agent: pane.agent,
                    });
                }
            }
        }
        state
    }

    #[test]
    fn test_status_counts() {
        let groups = vec![RepoGroup {
            id: "/tmp/project".into(),
            name: "project".into(),
            has_focus: false,
            panes: vec![
                (make_pane(1, PaneStatus::Running), make_git_info()),
                (make_pane(2, PaneStatus::Running), make_git_info()),
                (make_pane(3, PaneStatus::Idle), make_git_info()),
                (make_pane(4, PaneStatus::Error), make_git_info()),
                (make_pane(5, PaneStatus::Waiting), make_git_info()),
            ],
        }];
        let state = make_state_with_groups(groups);
        let (all, running, waiting, idle, error) = state.status_counts();
        assert_eq!(all, 5);
        assert_eq!(running, 2);
        assert_eq!(waiting, 1);
        assert_eq!(idle, 1);
        assert_eq!(error, 1);
    }

    #[test]
    fn test_status_counts_with_repo_filter() {
        let groups = vec![
            RepoGroup {
                id: "/tmp/project-a".into(),
                name: "project-a".into(),
                has_focus: false,
                panes: vec![(make_pane(1, PaneStatus::Running), make_git_info())],
            },
            RepoGroup {
                id: "/tmp/project-b".into(),
                name: "project-b".into(),
                has_focus: false,
                panes: vec![
                    (make_pane(2, PaneStatus::Idle), make_git_info()),
                    (make_pane(3, PaneStatus::Error), make_git_info()),
                ],
            },
        ];
        let mut state = make_state_with_groups(groups);
        state.repo_filter = RepoFilter::Repo("/tmp/project-b".into());

        let (all, running, _waiting, idle, error) = state.status_counts();
        assert_eq!(all, 2);
        assert_eq!(running, 0);
        assert_eq!(idle, 1);
        assert_eq!(error, 1);
    }

    #[test]
    fn test_select_prev_next() {
        let groups = vec![RepoGroup {
            id: "/tmp/project".into(),
            name: "project".into(),
            has_focus: false,
            panes: vec![
                (make_pane(1, PaneStatus::Running), make_git_info()),
                (make_pane(2, PaneStatus::Idle), make_git_info()),
                (make_pane(3, PaneStatus::Waiting), make_git_info()),
            ],
        }];
        let mut state = make_state_with_groups(groups);
        assert_eq!(state.selected_agent_row, 0);

        state.select_next();
        assert_eq!(state.selected_agent_row, 1);
        state.select_next();
        assert_eq!(state.selected_agent_row, 2);
        state.select_next();
        // Should stay at 2 (boundary)
        assert_eq!(state.selected_agent_row, 2);

        state.select_prev();
        assert_eq!(state.selected_agent_row, 1);
        state.select_prev();
        assert_eq!(state.selected_agent_row, 0);
        state.select_prev();
        // Should stay at 0 (boundary)
        assert_eq!(state.selected_agent_row, 0);
    }

    #[test]
    fn test_selected_pane() {
        let groups = vec![RepoGroup {
            id: "/tmp/project".into(),
            name: "project".into(),
            has_focus: false,
            panes: vec![
                (make_pane(10, PaneStatus::Running), make_git_info()),
                (make_pane(20, PaneStatus::Idle), make_git_info()),
            ],
        }];
        let mut state = make_state_with_groups(groups);

        assert_eq!(state.selected_pane().unwrap().pane_id, 10);
        state.select_next();
        assert_eq!(state.selected_pane().unwrap().pane_id, 20);
    }

    #[test]
    fn test_selected_pane_empty() {
        let state = make_state_with_groups(vec![]);
        assert!(state.selected_pane().is_none());
    }

    #[test]
    fn test_find_pane() {
        let groups = vec![RepoGroup {
            id: "/tmp/project".into(),
            name: "project".into(),
            has_focus: false,
            panes: vec![
                (make_pane(10, PaneStatus::Running), make_git_info()),
                (make_pane(20, PaneStatus::Idle), make_git_info()),
            ],
        }];
        let state = make_state_with_groups(groups);

        assert_eq!(state.find_pane(10).unwrap().pane_id, 10);
        assert_eq!(state.find_pane(20).unwrap().status, PaneStatus::Idle);
        assert!(state.find_pane(99).is_none());
    }

    #[test]
    fn test_repo_entries() {
        let groups = vec![
            RepoGroup {
                id: "/tmp/alpha".into(),
                name: "alpha".into(),
                has_focus: false,
                panes: vec![(make_pane(1, PaneStatus::Running), make_git_info())],
            },
            RepoGroup {
                id: "/tmp/beta".into(),
                name: "beta".into(),
                has_focus: true,
                panes: vec![(make_pane(2, PaneStatus::Idle), make_git_info())],
            },
        ];
        let state = make_state_with_groups(groups);
        let entries = state.repo_entries();
        assert_eq!(
            entries,
            vec![
                ("/tmp/alpha".to_string(), "alpha".to_string()),
                ("/tmp/beta".to_string(), "beta".to_string()),
            ]
        );
    }

    #[test]
    fn test_repo_filter_with_same_basename() {
        let groups = vec![
            RepoGroup {
                id: "/home/user/foo/api".into(),
                name: "foo/api".into(),
                has_focus: false,
                panes: vec![(make_pane(1, PaneStatus::Running), make_git_info())],
            },
            RepoGroup {
                id: "/home/user/bar/api".into(),
                name: "bar/api".into(),
                has_focus: false,
                panes: vec![
                    (make_pane(2, PaneStatus::Idle), make_git_info()),
                    (make_pane(3, PaneStatus::Error), make_git_info()),
                ],
            },
        ];
        let mut state = make_state_with_groups(groups);

        // Filter to foo/api by id
        state.repo_filter = RepoFilter::Repo("/home/user/foo/api".into());
        state.rebuild_row_targets();

        let (all, running, _, _, _) = state.status_counts();
        assert_eq!(all, 1);
        assert_eq!(running, 1);

        // Filter to bar/api by id
        state.repo_filter = RepoFilter::Repo("/home/user/bar/api".into());
        state.rebuild_row_targets();

        let (all, _, _, idle, error) = state.status_counts();
        assert_eq!(all, 2);
        assert_eq!(idle, 1);
        assert_eq!(error, 1);
    }

    #[test]
    fn test_repo_filter_validation_uses_id() {
        let groups = vec![RepoGroup {
            id: "/home/user/project".into(),
            name: "project".into(),
            has_focus: false,
            panes: vec![(make_pane(1, PaneStatus::Running), make_git_info())],
        }];
        let mut state = make_state_with_groups(groups);

        // Set a filter with a non-existent id
        state.repo_filter = RepoFilter::Repo("/nonexistent".into());
        state.rebuild_row_targets();

        // Should have been reset to All
        assert_eq!(state.repo_filter, RepoFilter::All);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::group::{PaneGitInfo, RepoGroup};
    use crate::wezterm::{AgentType, PaneInfo, PaneStatus, PermissionMode, WorkspaceInfo};

    fn make_pane(pane_id: u64, status: PaneStatus) -> PaneInfo {
        PaneInfo {
            pane_id,
            tab_id: 0,
            window_id: 0,
            workspace: "default".into(),
            pane_active: false,
            status,
            attention: false,
            agent: AgentType::Claude,
            path: "/tmp/test".into(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            session_started_at: None,
            turn_started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: Vec::new(),
        }
    }

    fn make_pane_with_path(pane_id: u64, status: PaneStatus, path: &str) -> PaneInfo {
        let mut pane = make_pane(pane_id, status);
        pane.path = path.into();
        pane
    }

    fn make_cached_activity(label: &str, progress: TaskProgress) -> CachedActivity {
        CachedActivity {
            fingerprint: LogFingerprint::missing(),
            snapshot: ActivitySnapshot {
                display_entries: vec![ActivityEntry {
                    timestamp: "14:32".into(),
                    tool: "Bash".into(),
                    label: label.into(),
                }],
                task_progress: progress,
            },
        }
    }

    #[test]
    fn test_refresh_local_views_updates_filters_without_snapshot_refresh() {
        let pane1 = make_pane_with_path(1, PaneStatus::Running, "/repo-a");
        let pane2 = make_pane_with_path(2, PaneStatus::Idle, "/repo-b");
        let mut state = AppState::new(999);
        state.repo_groups = vec![
            RepoGroup {
                id: "/repo-a".into(),
                name: "repo-a".into(),
                has_focus: true,
                panes: vec![(
                    pane1.clone(),
                    PaneGitInfo {
                        repo_root: Some("/repo-a".into()),
                        branch: Some("main".into()),
                        is_worktree: false,
                    },
                )],
            },
            RepoGroup {
                id: "/repo-b".into(),
                name: "repo-b".into(),
                has_focus: false,
                panes: vec![(
                    pane2.clone(),
                    PaneGitInfo {
                        repo_root: Some("/repo-b".into()),
                        branch: Some("main".into()),
                        is_worktree: false,
                    },
                )],
            },
        ];
        state.focused_pane_id = Some(1);
        state.activity_cache.insert(
            1,
            make_cached_activity(
                "focused activity",
                TaskProgress {
                    tasks: vec![("1".into(), activity::TaskStatus::InProgress)],
                },
            ),
        );
        state.activity_cache.insert(
            2,
            make_cached_activity(
                "other activity",
                TaskProgress {
                    tasks: vec![("2".into(), activity::TaskStatus::Completed)],
                },
            ),
        );

        let git_path = state.refresh_local_views();
        assert_eq!(git_path.as_deref(), Some("/repo-a"));
        assert_eq!(state.agent_row_targets.len(), 2);
        assert_eq!(state.activity_entries.len(), 1);
        assert_eq!(state.activity_entries[0].label, "focused activity");
        assert_eq!(state.pane_task_progress.len(), 2);

        state.repo_filter = RepoFilter::Repo("/repo-b".into());

        let git_path = state.refresh_local_views();
        assert_eq!(git_path, None);
        assert_eq!(state.agent_row_targets.len(), 1);
        assert_eq!(state.agent_row_targets[0].pane_id, 2);
        assert_eq!(state.activity_entries[0].label, "focused activity");
        assert_eq!(state.pane_task_progress.len(), 1);
        assert!(state.pane_task_progress.contains_key(&2));
        assert!(state.activity_cache.contains_key(&1));
        assert!(state.activity_cache.contains_key(&2));
    }

    #[test]
    fn test_refresh_local_views_ignores_stale_focused_pane() {
        let pane1 = make_pane_with_path(1, PaneStatus::Running, "/repo-a");
        let mut state = AppState::new(999);
        state.repo_groups = vec![RepoGroup {
            id: "/repo-a".into(),
            name: "repo-a".into(),
            has_focus: true,
            panes: vec![(
                pane1,
                PaneGitInfo {
                    repo_root: Some("/repo-a".into()),
                    branch: Some("main".into()),
                    is_worktree: false,
                },
            )],
        }];
        state.focused_pane_id = Some(42);
        state.activity_cache.insert(
            1,
            make_cached_activity(
                "visible activity",
                TaskProgress {
                    tasks: vec![("1".into(), activity::TaskStatus::InProgress)],
                },
            ),
        );
        state.activity_cache.insert(
            42,
            make_cached_activity(
                "stale activity",
                TaskProgress {
                    tasks: vec![("2".into(), activity::TaskStatus::Completed)],
                },
            ),
        );

        let git_path = state.refresh_local_views();

        assert_eq!(git_path, None);
        assert_eq!(state.activity_entries.len(), 1);
        assert_eq!(state.activity_entries[0].label, "visible activity");
        assert_eq!(state.pane_task_progress.len(), 1);
        assert!(state.pane_task_progress.contains_key(&1));
        assert!(!state.activity_cache.contains_key(&42));
    }

    #[test]
    fn test_apply_repo_info_updates_retargets_git_path_to_repo_root() {
        let pane = make_pane_with_path(1, PaneStatus::Running, "/repo-a/src");
        let mut state = AppState::new(999);
        state.workspaces = vec![WorkspaceInfo {
            workspace_name: "default".into(),
            tabs: vec![crate::wezterm::TabInfo {
                tab_id: 1,
                tab_title: "tab".into(),
                tab_active: true,
                panes: vec![pane.clone()],
            }],
        }];
        state.focused_pane_id = Some(1);
        state.repo_groups = vec![RepoGroup {
            id: "/repo-a/src".into(),
            name: "src".into(),
            has_focus: true,
            panes: vec![(pane, PaneGitInfo::default())],
        }];
        state.last_git_path = Some("/repo-a/src".into());

        let actions = state.apply_repo_info_updates(vec![RepoInfoUpdate {
            path: "/repo-a/src".into(),
            info: PaneGitInfo {
                repo_root: Some("/repo-a".into()),
                branch: Some("main".into()),
                is_worktree: false,
            },
        }]);

        assert_eq!(actions.git_path.as_deref(), Some("/repo-a"));
        assert!(actions.repo_paths.is_empty());
        assert_eq!(state.repo_groups.len(), 1);
        assert_eq!(state.repo_groups[0].id, "/repo-a");
        assert_eq!(state.repo_groups[0].name, "repo-a");
    }
}
