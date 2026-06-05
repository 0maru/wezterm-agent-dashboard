use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const GIT_LOOP_INTERVAL: Duration = Duration::from_millis(250);
const GIT_SUMMARY_TTL: Duration = Duration::from_secs(2);
const GIT_PR_TTL: Duration = Duration::from_secs(30);
const GIT_PR_LAZY_DELAY: Duration = Duration::from_secs(3);
const GIT_PR_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy)]
struct GitPollConfig {
    loop_interval: Duration,
    summary_ttl: Duration,
    pr_ttl: Duration,
    pr_lazy_delay: Duration,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileStatus {
    pub file: String,
    pub insertions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone)]
struct CachedGitData {
    data: GitData,
    summary_fetched_at: Option<Instant>,
    pr_fetched_at: Option<Instant>,
}

// ---------------------------------------------------------------------------
// Fetch git data
// ---------------------------------------------------------------------------

/// Fetch all git data for a given working directory.
pub fn fetch_git_data(cwd: &str) -> GitData {
    let mut data = fetch_git_summary(cwd);
    data.pr_number = fetch_pr_number(cwd);
    data
}

fn fetch_git_summary(cwd: &str) -> GitData {
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

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> Option<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::null());

    let started_at = Instant::now();
    let mut child = command.spawn().ok()?;

    loop {
        if child.try_wait().ok()?.is_some() {
            return child.wait_with_output().ok();
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_numstat(
    numstat: &str,
    files: &mut [GitFileStatus],
    total_ins: &mut u32,
    total_del: &mut u32,
) {
    let file_indexes: std::collections::HashMap<String, usize> = files
        .iter()
        .enumerate()
        .map(|(idx, file)| (file.file.clone(), idx))
        .collect();

    for line in numstat.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let ins = parts[0].parse::<u32>().unwrap_or(0);
            let del = parts[1].parse::<u32>().unwrap_or(0);
            let file = parts[2];

            *total_ins += ins;
            *total_del += del;

            if let Some(idx) = file_indexes.get(file) {
                let f = &mut files[*idx];
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
    let mut command = Command::new("gh");
    command
        .args(["pr", "view", "--json", "number", "-q", ".number"])
        .current_dir(cwd);

    let output = command_output_with_timeout(command, GIT_PR_COMMAND_TIMEOUT)?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse::<u32>().ok()
}

impl CachedGitData {
    fn new(path: &str) -> Self {
        Self {
            data: GitData {
                path: path.to_string(),
                ..Default::default()
            },
            summary_fetched_at: None,
            pr_fetched_at: None,
        }
    }

    fn summary_is_fresh(&self, now: Instant, ttl: Duration) -> bool {
        self.summary_fetched_at
            .is_some_and(|fetched_at| now.duration_since(fetched_at) < ttl)
    }

    fn pr_is_fresh(&self, now: Instant, ttl: Duration) -> bool {
        self.pr_fetched_at
            .is_some_and(|fetched_at| now.duration_since(fetched_at) < ttl)
    }
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
    mpsc::Sender<String>,
    Arc<AtomicBool>,
    thread::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel();
    let (path_tx, path_rx) = mpsc::channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let handle = thread::spawn(move || {
        run_git_poll_loop(
            active,
            path_rx,
            tx,
            shutdown_clone,
            GitPollConfig {
                loop_interval: GIT_LOOP_INTERVAL,
                summary_ttl: GIT_SUMMARY_TTL,
                pr_ttl: GIT_PR_TTL,
                pr_lazy_delay: GIT_PR_LAZY_DELAY,
            },
            fetch_git_summary,
            fetch_pr_number,
        );
    });

    (rx, path_tx, shutdown, handle)
}

fn run_git_poll_loop<FS, FP>(
    active: Arc<AtomicBool>,
    path_rx: mpsc::Receiver<String>,
    tx: mpsc::Sender<GitData>,
    shutdown: Arc<AtomicBool>,
    config: GitPollConfig,
    mut fetch_summary: FS,
    mut fetch_pr: FP,
) where
    FS: FnMut(&str) -> GitData,
    FP: FnMut(&str) -> Option<u32>,
{
    let mut current_path = String::new();
    let mut current_path_since = Instant::now();
    let mut cache = std::collections::HashMap::new();
    let mut last_sent: Option<GitData> = None;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        while let Ok(path) = path_rx.try_recv() {
            if current_path != path {
                current_path = path;
                current_path_since = Instant::now();
                last_sent = None;
            }
        }

        if !active.load(Ordering::Relaxed) {
            thread::sleep(config.loop_interval);
            continue;
        }

        let data = current_git_data_with(
            &mut cache,
            &current_path,
            current_path_since,
            Instant::now(),
            config,
            &mut fetch_summary,
            &mut fetch_pr,
        );
        if last_sent.as_ref() != Some(&data) && tx.send(data.clone()).is_err() {
            break;
        }
        last_sent = Some(data);

        thread::sleep(config.loop_interval);
    }
}

fn current_git_data_with<FS, FP>(
    cache: &mut std::collections::HashMap<String, CachedGitData>,
    path: &str,
    path_selected_at: Instant,
    now: Instant,
    config: GitPollConfig,
    fetch_summary: &mut FS,
    fetch_pr: &mut FP,
) -> GitData
where
    FS: FnMut(&str) -> GitData,
    FP: FnMut(&str) -> Option<u32>,
{
    if path.is_empty() {
        return GitData::default();
    }

    let entry = cache
        .entry(path.to_string())
        .or_insert_with(|| CachedGitData::new(path));

    if !entry.summary_is_fresh(now, config.summary_ttl) {
        let cached_pr = if entry.pr_is_fresh(now, config.pr_ttl) {
            entry.data.pr_number
        } else {
            None
        };

        entry.data = fetch_summary(path);
        entry.data.pr_number = cached_pr;
        entry.summary_fetched_at = Some(now);
    }

    if should_fetch_pr(
        entry,
        now,
        path_selected_at,
        config.pr_ttl,
        config.pr_lazy_delay,
    ) {
        entry.data.pr_number = fetch_pr(path);
        entry.pr_fetched_at = Some(now);
    }

    entry.data.clone()
}

fn should_fetch_pr(
    entry: &CachedGitData,
    now: Instant,
    path_selected_at: Instant,
    pr_ttl: Duration,
    pr_lazy_delay: Duration,
) -> bool {
    if entry.pr_is_fresh(now, pr_ttl) {
        return false;
    }

    if now.duration_since(path_selected_at) < pr_lazy_delay {
        return false;
    }

    !entry.data.branch.is_empty() && !entry.data.remote_url.is_empty()
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

    #[test]
    fn test_command_output_with_timeout_returns_output() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 42"]);

        let output = command_output_with_timeout(command, Duration::from_secs(1)).unwrap();

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "42");
    }

    #[test]
    fn test_command_output_with_timeout_kills_slow_command() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 1; printf 42"]);

        let output = command_output_with_timeout(command, Duration::from_millis(20));

        assert!(output.is_none());
    }

    #[test]
    fn test_should_fetch_pr_waits_for_lazy_delay() {
        let now = Instant::now();
        let entry = CachedGitData {
            data: GitData {
                branch: "main".into(),
                remote_url: "https://github.com/user/repo".into(),
                ..Default::default()
            },
            summary_fetched_at: Some(now),
            pr_fetched_at: None,
        };

        assert!(!should_fetch_pr(
            &entry,
            now,
            now,
            GIT_PR_TTL,
            GIT_PR_LAZY_DELAY
        ));
        assert!(should_fetch_pr(
            &entry,
            now + GIT_PR_LAZY_DELAY,
            now,
            GIT_PR_TTL,
            GIT_PR_LAZY_DELAY
        ));
    }

    #[test]
    fn test_should_fetch_pr_respects_fresh_cache() {
        let now = Instant::now();
        let entry = CachedGitData {
            data: GitData {
                branch: "main".into(),
                remote_url: "https://github.com/user/repo".into(),
                pr_number: Some(123),
                ..Default::default()
            },
            summary_fetched_at: Some(now),
            pr_fetched_at: Some(now),
        };

        assert!(!should_fetch_pr(
            &entry,
            now + Duration::from_secs(1),
            now - GIT_PR_LAZY_DELAY,
            GIT_PR_TTL,
            GIT_PR_LAZY_DELAY
        ));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    #[test]
    fn test_git_worker_waits_until_active() {
        let (tx, rx) = mpsc::channel();
        let (path_tx, path_rx) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        path_tx.send("/repo".into()).unwrap();

        let active_clone = active.clone();
        let shutdown_clone = shutdown.clone();
        let handle = thread::spawn(move || {
            run_git_poll_loop(
                active_clone,
                path_rx,
                tx,
                shutdown_clone,
                GitPollConfig {
                    loop_interval: Duration::from_millis(5),
                    summary_ttl: Duration::from_secs(60),
                    pr_ttl: Duration::from_secs(60),
                    pr_lazy_delay: Duration::from_secs(60),
                },
                |path| GitData {
                    branch: "main".into(),
                    path: path.into(),
                    ..Default::default()
                },
                |_| None,
            );
        });

        assert!(rx.recv_timeout(Duration::from_millis(40)).is_err());
        active.store(true, Ordering::Relaxed);

        let data = rx.recv_timeout(Duration::from_millis(100)).unwrap();

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        assert_eq!(data.path, "/repo");
        assert_eq!(data.branch, "main");
    }

    #[test]
    fn test_git_worker_uses_latest_queued_path() {
        let (tx, rx) = mpsc::channel();
        let (path_tx, path_rx) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(AtomicBool::new(false));
        let seen_paths = Arc::new(std::sync::Mutex::new(Vec::new()));

        path_tx.send("/repo-old".into()).unwrap();
        path_tx.send("/repo-new".into()).unwrap();

        let active_clone = active.clone();
        let shutdown_clone = shutdown.clone();
        let seen_paths_clone = seen_paths.clone();
        let handle = thread::spawn(move || {
            run_git_poll_loop(
                active_clone,
                path_rx,
                tx,
                shutdown_clone,
                GitPollConfig {
                    loop_interval: Duration::from_millis(5),
                    summary_ttl: Duration::from_secs(60),
                    pr_ttl: Duration::from_secs(60),
                    pr_lazy_delay: Duration::from_secs(60),
                },
                move |path| {
                    seen_paths_clone.lock().unwrap().push(path.to_string());
                    GitData {
                        branch: "main".into(),
                        path: path.into(),
                        ..Default::default()
                    }
                },
                |_| None,
            );
        });

        let data = rx.recv_timeout(Duration::from_millis(100)).unwrap();

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        assert_eq!(data.path, "/repo-new");
        assert_eq!(*seen_paths.lock().unwrap(), vec!["/repo-new".to_string()]);
    }

    #[test]
    fn test_git_worker_emits_summary_then_lazy_pr_update() {
        let (tx, rx) = mpsc::channel();
        let (path_tx, path_rx) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(AtomicBool::new(false));
        let summary_calls = Arc::new(AtomicUsize::new(0));
        let pr_calls = Arc::new(AtomicUsize::new(0));

        path_tx.send("/repo".into()).unwrap();

        let active_clone = active.clone();
        let shutdown_clone = shutdown.clone();
        let summary_calls_clone = summary_calls.clone();
        let pr_calls_clone = pr_calls.clone();
        let handle = thread::spawn(move || {
            run_git_poll_loop(
                active_clone,
                path_rx,
                tx,
                shutdown_clone,
                GitPollConfig {
                    loop_interval: Duration::from_millis(5),
                    summary_ttl: Duration::from_secs(60),
                    pr_ttl: Duration::from_secs(60),
                    pr_lazy_delay: Duration::from_millis(30),
                },
                move |path| {
                    summary_calls_clone.fetch_add(1, AtomicOrdering::Relaxed);
                    GitData {
                        branch: "main".into(),
                        remote_url: "https://github.com/user/repo".into(),
                        path: path.into(),
                        ..Default::default()
                    }
                },
                move |_| {
                    pr_calls_clone.fetch_add(1, AtomicOrdering::Relaxed);
                    Some(42)
                },
            );
        });

        let first = rx.recv_timeout(Duration::from_millis(100)).unwrap();
        let second = rx.recv_timeout(Duration::from_millis(150)).unwrap();

        shutdown.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        assert_eq!(first.path, "/repo");
        assert_eq!(first.pr_number, None);
        assert_eq!(second.path, "/repo");
        assert_eq!(second.pr_number, Some(42));
        assert_eq!(summary_calls.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(pr_calls.load(AtomicOrdering::Relaxed), 1);
    }
}
