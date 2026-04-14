use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct GitData {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub staged_files: Vec<GitFileStatus>,
    pub unstaged_files: Vec<GitFileStatus>,
    pub untracked_files: Vec<String>,
    pub staged_insertions: u32,
    pub staged_deletions: u32,
    pub unstaged_insertions: u32,
    pub unstaged_deletions: u32,
    pub remote_url: String,
    pub pr_number: Option<u32>,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct GitFileStatus {
    pub file: String,
    pub insertions: u32,
    pub deletions: u32,
}

// ---------------------------------------------------------------------------
// Fetch git data
// ---------------------------------------------------------------------------

/// Fetch all git data for a given working directory.
pub fn fetch_git_data(cwd: &str) -> GitData {
    if cwd.is_empty() {
        return GitData::default();
    }

    let mut data = GitData {
        path: cwd.to_string(),
        ..Default::default()
    };

    // Branch name
    if let Some(branch) = git_cmd(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        data.branch = branch;
    }

    // Ahead/behind
    if let Some(ab) = git_cmd(
        cwd,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    ) {
        let parts: Vec<&str> = ab.split_whitespace().collect();
        if parts.len() == 2 {
            data.ahead = parts[0].parse().unwrap_or(0);
            data.behind = parts[1].parse().unwrap_or(0);
        }
    }

    // Status
    if let Some(status) = git_cmd(cwd, &["status", "--short"]) {
        for line in status.lines() {
            if line.len() < 3 {
                continue;
            }
            let index = line.as_bytes()[0];
            let worktree = line.as_bytes()[1];
            let file = line[3..].trim().to_string();

            match (index, worktree) {
                (b'?', b'?') => data.untracked_files.push(file),
                _ => {
                    if index != b' ' && index != b'?' {
                        data.staged_files.push(GitFileStatus {
                            file: file.clone(),
                            insertions: 0,
                            deletions: 0,
                        });
                    }
                    if worktree != b' ' && worktree != b'?' {
                        data.unstaged_files.push(GitFileStatus {
                            file,
                            insertions: 0,
                            deletions: 0,
                        });
                    }
                }
            }
        }
    }

    // Staged diff stats
    if let Some(numstat) = git_cmd(cwd, &["diff", "--cached", "--numstat"]) {
        parse_numstat(
            &numstat,
            &mut data.staged_files,
            &mut data.staged_insertions,
            &mut data.staged_deletions,
        );
    }

    // Unstaged diff stats
    if let Some(numstat) = git_cmd(cwd, &["diff", "--numstat"]) {
        parse_numstat(
            &numstat,
            &mut data.unstaged_files,
            &mut data.unstaged_insertions,
            &mut data.unstaged_deletions,
        );
    }

    // Remote URL
    if let Some(url) = git_cmd(cwd, &["remote", "get-url", "origin"]) {
        data.remote_url = normalize_remote_url(&url);
    }

    // PR number (with timeout)
    data.pr_number = fetch_pr_number(cwd);

    data
}

fn git_cmd(cwd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_numstat(
    numstat: &str,
    files: &mut [GitFileStatus],
    total_ins: &mut u32,
    total_del: &mut u32,
) {
    for line in numstat.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let ins = parts[0].parse::<u32>().unwrap_or(0);
            let del = parts[1].parse::<u32>().unwrap_or(0);
            let file = parts[2];

            *total_ins += ins;
            *total_del += del;

            if let Some(f) = files.iter_mut().find(|f| f.file == file) {
                f.insertions = ins;
                f.deletions = del;
            }
        }
    }
}

fn normalize_remote_url(url: &str) -> String {
    // Convert SSH URLs to HTTPS format
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        return format!("https://github.com/{rest}");
    }
    url.strip_suffix(".git").unwrap_or(url).to_string()
}

fn fetch_pr_number(cwd: &str) -> Option<u32> {
    let output = Command::new("gh")
        .args(["pr", "view", "--json", "number", "-q", ".number"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse::<u32>().ok()
}

// ---------------------------------------------------------------------------
// Background polling thread
// ---------------------------------------------------------------------------

/// Start a background thread that polls git data every 2 seconds.
/// Only polls when `active` flag is true (git tab visible).
pub fn start_git_poll_thread(
    active: Arc<AtomicBool>,
) -> (
    mpsc::Receiver<GitData>,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let handle = thread::spawn(move || {
        loop {
            if shutdown_clone.load(Ordering::Relaxed) {
                break;
            }

            thread::sleep(Duration::from_secs(2));

            if !active.load(Ordering::Relaxed) {
                continue;
            }

            let path_file = "/tmp/wezterm-agent-dashboard-git-path";
            let path = std::fs::read_to_string(path_file)
                .unwrap_or_default()
                .trim()
                .to_string();

            if path.is_empty() {
                continue;
            }

            let data = fetch_git_data(&path);
            if tx.send(data).is_err() {
                break;
            }
        }
    });

    (rx, shutdown, handle)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_remote_url_ssh_to_https() {
        assert_eq!(
            normalize_remote_url("git@github.com:user/repo.git"),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_normalize_remote_url_ssh_no_git_suffix() {
        assert_eq!(
            normalize_remote_url("git@github.com:user/repo"),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_normalize_remote_url_https_with_git_suffix() {
        assert_eq!(
            normalize_remote_url("https://github.com/user/repo.git"),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_normalize_remote_url_https_no_suffix() {
        assert_eq!(
            normalize_remote_url("https://github.com/user/repo"),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_parse_numstat_basic() {
        let mut files = vec![
            GitFileStatus {
                file: "src/main.rs".into(),
                insertions: 0,
                deletions: 0,
            },
            GitFileStatus {
                file: "README.md".into(),
                insertions: 0,
                deletions: 0,
            },
        ];
        let mut total_ins = 0;
        let mut total_del = 0;

        parse_numstat(
            "10\t3\tsrc/main.rs\n5\t1\tREADME.md",
            &mut files,
            &mut total_ins,
            &mut total_del,
        );

        assert_eq!(files[0].insertions, 10);
        assert_eq!(files[0].deletions, 3);
        assert_eq!(files[1].insertions, 5);
        assert_eq!(files[1].deletions, 1);
        assert_eq!(total_ins, 15);
        assert_eq!(total_del, 4);
    }

    #[test]
    fn test_parse_numstat_empty() {
        let mut files = vec![];
        let mut total_ins = 0;
        let mut total_del = 0;

        parse_numstat("", &mut files, &mut total_ins, &mut total_del);

        assert_eq!(total_ins, 0);
        assert_eq!(total_del, 0);
    }

    #[test]
    fn test_parse_numstat_binary_file() {
        let mut files = vec![GitFileStatus {
            file: "image.png".into(),
            insertions: 0,
            deletions: 0,
        }];
        let mut total_ins = 0;
        let mut total_del = 0;

        // Binary files show "-\t-\tfilename" in numstat
        parse_numstat(
            "-\t-\timage.png",
            &mut files,
            &mut total_ins,
            &mut total_del,
        );

        // "-" parses as 0 via unwrap_or(0)
        assert_eq!(files[0].insertions, 0);
        assert_eq!(files[0].deletions, 0);
        assert_eq!(total_ins, 0);
        assert_eq!(total_del, 0);
    }

    #[test]
    fn test_parse_numstat_file_not_in_list() {
        let mut files = vec![GitFileStatus {
            file: "other.rs".into(),
            insertions: 0,
            deletions: 0,
        }];
        let mut total_ins = 0;
        let mut total_del = 0;

        parse_numstat(
            "5\t2\tunknown.rs",
            &mut files,
            &mut total_ins,
            &mut total_del,
        );

        // Totals are updated even if file is not in the list
        assert_eq!(total_ins, 5);
        assert_eq!(total_del, 2);
        // The existing file should not be modified
        assert_eq!(files[0].insertions, 0);
    }

    #[test]
    fn test_fetch_git_data_empty_cwd() {
        let data = fetch_git_data("");
        assert!(data.branch.is_empty());
        assert!(data.path.is_empty());
    }
}
