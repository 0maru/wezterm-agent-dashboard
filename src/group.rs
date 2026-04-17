use crate::wezterm::{PaneInfo, WorkspaceInfo};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
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

fn cached_git_info_for_path(
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

fn resolve_cached_git_info(
    path: &str,
    git_cache: &mut HashMap<String, PaneGitInfo>,
) -> PaneGitInfo {
    if let Some(info) = cached_git_info_for_path(path, git_cache) {
        git_cache.insert(path.to_string(), info.clone());
        return info;
    }

    let info = resolve_pane_git_info(path);
    git_cache.insert(path.to_string(), info.clone());
    info
}

// ---------------------------------------------------------------------------
// Grouping
// ---------------------------------------------------------------------------

/// Group panes by repository root across all workspaces.
/// Panes in the same git repo (including worktrees) are grouped together.
pub fn group_panes_by_repo(
    workspaces: &[WorkspaceInfo],
    focused_pane_id: Option<u64>,
    git_cache: &mut HashMap<String, PaneGitInfo>,
) -> Vec<RepoGroup> {
    // group_key → Vec<(PaneInfo, PaneGitInfo)>
    let mut groups: IndexMap<String, Vec<(PaneInfo, PaneGitInfo)>> = IndexMap::new();

    for ws in workspaces {
        for tab in &ws.tabs {
            for pane in &tab.panes {
                let git_info = resolve_cached_git_info(&pane.path, git_cache);

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
mod tests {
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
            pane_pid: None,
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

        let info = resolve_cached_git_info("/repo/src/module", &mut cache);

        assert_eq!(info.repo_root.as_deref(), Some("/repo"));
        assert!(cache.contains_key("/repo/src/module"));
    }

    #[test]
    fn test_group_panes_by_repo_reuses_cached_git_info_for_subdirs() {
        let workspaces = make_workspace(vec![make_pane(1, "/repo"), make_pane(2, "/repo/src")]);
        let mut cache = HashMap::new();
        cache.insert("/repo".into(), make_git_info("/repo"));

        let groups = group_panes_by_repo(&workspaces, Some(2), &mut cache);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "repo");
        assert!(groups[0].has_focus);
        assert_eq!(groups[0].panes.len(), 2);
        assert_eq!(groups[0].panes[0].1.repo_root.as_deref(), Some("/repo"));
        assert_eq!(groups[0].panes[1].1.repo_root.as_deref(), Some("/repo"));
        assert!(cache.contains_key("/repo/src"));
    }
}
