use crate::wezterm::{PaneInfo, WorkspaceInfo};
use indexmap::IndexMap;
use std::collections::HashMap;
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

// ---------------------------------------------------------------------------
// Grouping
// ---------------------------------------------------------------------------

/// Group panes by repository root across all workspaces.
/// Panes in the same git repo (including worktrees) are grouped together.
pub fn group_panes_by_repo(
    workspaces: &[WorkspaceInfo],
    focused_pane_id: Option<u64>,
) -> Vec<RepoGroup> {
    let mut git_cache: HashMap<String, PaneGitInfo> = HashMap::new();
    // group_key → Vec<(PaneInfo, PaneGitInfo)>
    let mut groups: IndexMap<String, Vec<(PaneInfo, PaneGitInfo)>> = IndexMap::new();

    for ws in workspaces {
        for tab in &ws.tabs {
            for pane in &tab.panes {
                let git_info = git_cache
                    .entry(pane.path.clone())
                    .or_insert_with(|| resolve_pane_git_info(&pane.path))
                    .clone();

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
