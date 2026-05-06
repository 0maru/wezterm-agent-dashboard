use crate::wezterm::{PaneInfo, WorkspaceInfo};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneGitInfo {
    pub repo_root: Option<String>,
    pub branch: Option<String>,
    pub is_worktree: bool,
}

#[derive(Debug, Clone)]
pub struct RepoGroup {
    pub id: String,
    pub name: String,
    pub has_focus: bool,
    pub panes: Vec<(PaneInfo, PaneGitInfo)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfoUpdate {
    pub path: String,
    pub info: PaneGitInfo,
}

// ---------------------------------------------------------------------------
// Git info resolution
// ---------------------------------------------------------------------------

/// Resolve git information for a path.
/// Uses `git rev-parse` to find the repo root, branch, and whether it's a worktree.
pub fn resolve_pane_git_info(path: &str) -> PaneGitInfo {
    if path.is_empty() {
        return PaneGitInfo {
            repo_root: None,
            branch: None,
            is_worktree: false,
        };
    }

    let output = Command::new("git")
        .args([
            "rev-parse",
            "--abbrev-ref",
            "HEAD",
            "--git-common-dir",
            "--git-dir",
        ])
        .current_dir(path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => {
            return PaneGitInfo {
                repo_root: None,
                branch: None,
                is_worktree: false,
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.len() < 3 {
        return PaneGitInfo {
            repo_root: None,
            branch: None,
            is_worktree: false,
        };
    }

    let branch = lines[0].to_string();
    let git_common_dir = lines[1];
    let git_dir = lines[2];

    // The repo root is the parent of --git-common-dir
    let repo_root = std::path::Path::new(path)
        .join(git_common_dir)
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_string_lossy().to_string()));

    // Detect worktree: if --git-common-dir != --git-dir, it's a worktree
    let common_canonical = std::path::Path::new(path)
        .join(git_common_dir)
        .canonicalize()
        .ok();
    let dir_canonical = std::path::Path::new(path).join(git_dir).canonicalize().ok();
    let is_worktree = match (common_canonical, dir_canonical) {
        (Some(c), Some(d)) => c != d,
        _ => false,
    };

    PaneGitInfo {
        repo_root,
        branch: Some(branch),
        is_worktree,
    }
}

pub fn lookup_cached_git_info_for_path(
    path: &str,
    git_cache: &HashMap<String, PaneGitInfo>,
) -> Option<PaneGitInfo> {
    if let Some(info) = git_cache.get(path) {
        return Some(info.clone());
    }

    let path = Path::new(path);

    git_cache
        .values()
        .filter_map(|info| {
            let repo_root = info.repo_root.as_ref()?;
            let repo_path = Path::new(repo_root);
            if path == repo_path || path.starts_with(repo_path) {
                Some((repo_root.len(), info.clone()))
            } else {
                None
            }
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, info)| info)
}

pub fn start_repo_poll_thread() -> (
    mpsc::Receiver<RepoInfoUpdate>,
    mpsc::Sender<String>,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel();
    let (path_tx, path_rx) = mpsc::channel::<String>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let handle = thread::spawn(move || {
        run_repo_poll_loop(
            tx,
            path_rx,
            shutdown_clone,
            Duration::from_millis(50),
            resolve_pane_git_info,
        );
    });

    (rx, path_tx, shutdown, handle)
}

fn run_repo_poll_loop<F>(
    tx: mpsc::Sender<RepoInfoUpdate>,
    path_rx: mpsc::Receiver<String>,
    shutdown: Arc<AtomicBool>,
    idle_sleep: Duration,
    mut resolve: F,
) where
    F: FnMut(&str) -> PaneGitInfo,
{
    let mut cache = HashMap::new();
    let mut queued_paths = VecDeque::new();
    let mut queued_set = HashSet::new();
    let mut release_after_drain: Option<String> = None;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        while let Ok(path) = path_rx.try_recv() {
            if queued_set.insert(path.clone()) {
                queued_paths.push_back(path);
            }
        }

        if let Some(path) = release_after_drain.take() {
            queued_set.remove(&path);
        }

        let Some(path) = queued_paths.pop_front() else {
            thread::sleep(idle_sleep);
            continue;
        };

        let info = if let Some(info) = lookup_cached_git_info_for_path(&path, &cache) {
            info
        } else {
            resolve(&path)
        };

        cache.insert(path.clone(), info.clone());
        if let Some(repo_root) = info.repo_root.as_ref() {
            cache
                .entry(repo_root.clone())
                .or_insert_with(|| info.clone());
        }

        if tx
            .send(RepoInfoUpdate {
                path: path.clone(),
                info,
            })
            .is_err()
        {
            break;
        }

        release_after_drain = Some(path);
    }
}

// ---------------------------------------------------------------------------
// Grouping
// ---------------------------------------------------------------------------

/// Group panes by repository root across all workspaces.
/// Panes in the same git repo (including worktrees) are grouped together.
pub fn group_panes_by_repo(
    workspaces: &[WorkspaceInfo],
    focused_pane_id: Option<u64>,
    git_cache: &HashMap<String, PaneGitInfo>,
) -> Vec<RepoGroup> {
    // group_key → Vec<(PaneInfo, PaneGitInfo)>
    let mut groups: IndexMap<String, Vec<(PaneInfo, PaneGitInfo)>> = IndexMap::new();

    for ws in workspaces {
        for tab in &ws.tabs {
            for pane in &tab.panes {
                let git_info =
                    lookup_cached_git_info_for_path(&pane.path, git_cache).unwrap_or_default();

                let group_key = git_info
                    .repo_root
                    .clone()
                    .unwrap_or_else(|| pane.path.clone());

                groups
                    .entry(group_key)
                    .or_default()
                    .push((pane.clone(), git_info));
            }
        }
    }

    let mut result: Vec<RepoGroup> = groups
        .into_iter()
        .map(|(key, panes)| {
            let name = std::path::Path::new(&key)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| key.clone());

            let has_focus =
                focused_pane_id.is_some_and(|fid| panes.iter().any(|(p, _)| p.pane_id == fid));

            RepoGroup {
                id: key,
                name,
                has_focus,
                panes,
            }
        })
        .collect();

    disambiguate_names(&mut result);

    result
}

fn disambiguate_names(groups: &mut [RepoGroup]) {
    let mut name_count: HashMap<String, usize> = HashMap::new();
    for g in groups.iter() {
        *name_count.entry(g.name.clone()).or_default() += 1;
    }

    for g in groups.iter_mut() {
        if name_count.get(&g.name).copied().unwrap_or(0) > 1 {
            let path = std::path::Path::new(&g.id);
            if let (Some(parent), Some(file_name)) =
                (path.parent().and_then(|p| p.file_name()), path.file_name())
            {
                g.name = format!(
                    "{}/{}",
                    parent.to_string_lossy(),
                    file_name.to_string_lossy()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_group(id: &str, name: &str) -> RepoGroup {
        RepoGroup {
            id: id.into(),
            name: name.into(),
            has_focus: false,
            panes: vec![],
        }
    }

    #[test]
    fn test_disambiguate_unique_names() {
        let mut groups = vec![
            make_test_group("/home/user/src/foo", "foo"),
            make_test_group("/home/user/src/bar", "bar"),
        ];
        disambiguate_names(&mut groups);
        assert_eq!(groups[0].name, "foo");
        assert_eq!(groups[1].name, "bar");
    }

    #[test]
    fn test_disambiguate_duplicate_names() {
        let mut groups = vec![
            make_test_group("/home/user/src/foo/api", "api"),
            make_test_group("/home/user/src/bar/api", "api"),
        ];
        disambiguate_names(&mut groups);
        assert_eq!(groups[0].name, "foo/api");
        assert_eq!(groups[1].name, "bar/api");
    }

    #[test]
    fn test_disambiguate_mixed() {
        let mut groups = vec![
            make_test_group("/home/user/src/foo/api", "api"),
            make_test_group("/home/user/src/bar/api", "api"),
            make_test_group("/home/user/src/web", "web"),
        ];
        disambiguate_names(&mut groups);
        assert_eq!(groups[0].name, "foo/api");
        assert_eq!(groups[1].name, "bar/api");
        assert_eq!(groups[2].name, "web");
    }
}

#[cfg(test)]
mod grouping_tests {
    use super::*;
    use crate::wezterm::{AgentType, PaneStatus, PermissionMode, TabInfo};

    fn make_pane(pane_id: u64, path: &str) -> PaneInfo {
        PaneInfo {
            pane_id,
            tab_id: 1,
            window_id: 1,
            workspace: "default".into(),
            pane_active: false,
            status: PaneStatus::Running,
            attention: false,
            agent: AgentType::Codex,
            path: path.into(),
            prompt: String::new(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: Vec::new(),
            usage: crate::usage::UsageStats::default(),
        }
    }

    fn make_workspace(panes: Vec<PaneInfo>) -> Vec<WorkspaceInfo> {
        vec![WorkspaceInfo {
            workspace_name: "default".into(),
            tabs: vec![TabInfo {
                tab_id: 1,
                tab_title: "tab".into(),
                tab_active: true,
                panes,
            }],
        }]
    }

    fn make_git_info(repo_root: &str) -> PaneGitInfo {
        PaneGitInfo {
            repo_root: Some(repo_root.into()),
            branch: Some("main".into()),
            is_worktree: false,
        }
    }

    #[test]
    fn test_cached_git_info_for_path_reuses_repo_root_prefix() {
        let mut cache = HashMap::new();
        cache.insert("/repo".into(), make_git_info("/repo"));

        let info = lookup_cached_git_info_for_path("/repo/src/module", &cache).unwrap();

        assert_eq!(info.repo_root.as_deref(), Some("/repo"));
    }

    #[test]
    fn test_group_panes_by_repo_reuses_cached_git_info_for_subdirs() {
        let workspaces = make_workspace(vec![make_pane(1, "/repo"), make_pane(2, "/repo/src")]);
        let mut cache = HashMap::new();
        cache.insert("/repo".into(), make_git_info("/repo"));

        let groups = group_panes_by_repo(&workspaces, Some(2), &cache);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "repo");
        assert!(groups[0].has_focus);
        assert_eq!(groups[0].panes.len(), 2);
        assert_eq!(groups[0].panes[0].1.repo_root.as_deref(), Some("/repo"));
        assert_eq!(groups[0].panes[1].1.repo_root.as_deref(), Some("/repo"));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn make_git_info(repo_root: &str) -> PaneGitInfo {
        PaneGitInfo {
            repo_root: Some(repo_root.into()),
            branch: Some("main".into()),
            is_worktree: false,
        }
    }

    #[test]
    fn test_repo_worker_reuses_cached_repo_info_for_subdirs() {
        use std::sync::atomic::AtomicUsize;

        let (tx, rx) = mpsc::channel();
        let (path_tx, path_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));

        let shutdown_clone = shutdown.clone();
        let calls_clone = calls.clone();
        let handle = thread::spawn(move || {
            run_repo_poll_loop(
                tx,
                path_rx,
                shutdown_clone,
                Duration::from_millis(1),
                move |_| {
                    calls_clone.fetch_add(1, Ordering::Relaxed);
                    make_git_info("/repo")
                },
            );
        });

        path_tx.send("/repo".into()).unwrap();
        let first = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(first.info.repo_root.as_deref(), Some("/repo"));

        path_tx.send("/repo/src/module".into()).unwrap();
        let second = rx.recv_timeout(Duration::from_millis(100)).unwrap();

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        assert_eq!(second.info.repo_root.as_deref(), Some("/repo"));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_repo_worker_dedupes_inflight_duplicate_paths() {
        let (tx, rx) = mpsc::channel();
        let (path_tx, path_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = mpsc::channel();

        let shutdown_clone = shutdown.clone();
        let handle = thread::spawn(move || {
            run_repo_poll_loop(
                tx,
                path_rx,
                shutdown_clone,
                Duration::from_millis(1),
                move |_| {
                    let _ = started_tx.send(());
                    thread::sleep(Duration::from_millis(40));
                    make_git_info("/repo")
                },
            );
        });

        path_tx.send("/repo".into()).unwrap();
        started_rx.recv_timeout(Duration::from_millis(100)).unwrap();
        path_tx.send("/repo".into()).unwrap();

        let first = rx.recv_timeout(Duration::from_millis(150)).unwrap();
        let second = rx.recv_timeout(Duration::from_millis(80));

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        assert_eq!(first.info.repo_root.as_deref(), Some("/repo"));
        assert!(second.is_err());
    }
}
