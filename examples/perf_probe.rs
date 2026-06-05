use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use wezterm_agent_dashboard::activity::{self, TaskProgress};
use wezterm_agent_dashboard::cli;
use wezterm_agent_dashboard::git;
use wezterm_agent_dashboard::group;
use wezterm_agent_dashboard::wezterm::{
    self, AgentType, PaneInfo, PaneStatus, PermissionMode, RawWezTermPane, TabInfo, WorkspaceInfo,
};

fn main() -> Result<(), Box<dyn Error>> {
    let bench_root = std::env::temp_dir().join(format!(
        "wezterm-agent-dashboard-perf-{}",
        std::process::id()
    ));
    if bench_root.exists() {
        fs::remove_dir_all(&bench_root)?;
    }
    fs::create_dir_all(&bench_root)?;

    let pane_ids: Vec<u64> = (91001_u64..91033).collect();

    let result = run(&bench_root, &pane_ids);

    for pane_id in &pane_ids {
        let _ = fs::remove_file(activity::log_file_path(*pane_id));
    }
    let _ = fs::remove_dir_all(&bench_root);

    result
}

fn run(bench_root: &Path, pane_ids: &[u64]) -> Result<(), Box<dyn Error>> {
    let repo_root = bench_root.join("repo");
    init_git_repo(&repo_root)?;
    let workspaces = make_workspaces(&repo_root, 48)?;
    write_activity_logs(pane_ids, 200)?;
    let cwd = std::env::current_dir()?;

    println!("Benchmark target: release-mode local perf probe");
    println!("Current repo path: {}", cwd.display());
    println!();

    bench("cli::local_time_hhmm", 20, || {
        black_box(cli::local_time_hhmm())
    });
    bench_command("cmd: ps -eo pid,args", 20, None, &["ps", "-eo", "pid,args"]);
    bench_command(
        "cmd: wezterm cli list --format json",
        10,
        None,
        &["wezterm", "cli", "list", "--format", "json"],
    );
    bench("wezterm::build_workspaces(120 panes)", 20, || {
        black_box(wezterm::build_workspaces(make_raw_panes(120), None))
    });
    bench_command(
        "cmd: git rev-parse(group path)",
        10,
        Some(repo_root.join("pane-0")),
        &[
            "git",
            "rev-parse",
            "--abbrev-ref",
            "HEAD",
            "--git-common-dir",
            "--git-dir",
        ],
    );
    bench("group::group_panes_by_repo(48 panes)", 5, || {
        let cache = std::collections::HashMap::new();
        black_box(group::group_panes_by_repo(&workspaces, Some(1), &cache))
    });
    bench("activity refresh simulation (32 panes)", 20, || {
        black_box(simulate_task_progress_refresh(pane_ids))
    });
    bench_command(
        "cmd: git status --short",
        10,
        Some(cwd.clone()),
        &["git", "status", "--short"],
    );
    bench_command(
        "cmd: git diff --numstat",
        10,
        Some(cwd.clone()),
        &["git", "diff", "--numstat"],
    );
    bench_command(
        "cmd: gh pr view --json number",
        3,
        Some(cwd.clone()),
        &["gh", "pr", "view", "--json", "number", "-q", ".number"],
    );
    bench("git::fetch_git_data(current repo)", 3, || {
        black_box(git::fetch_git_data(cwd.to_string_lossy().as_ref()))
    });

    Ok(())
}

fn bench<T, F>(name: &str, iterations: usize, mut f: F)
where
    F: FnMut() -> T,
{
    let _ = black_box(f());

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        let _ = black_box(f());
        samples.push(start.elapsed());
    }

    samples.sort_unstable();

    let total = samples
        .iter()
        .copied()
        .fold(Duration::ZERO, |acc, sample| acc + sample);
    let mean = total / iterations as u32;
    let median = samples[iterations / 2];
    let p95_index = ((iterations * 95) / 100).min(iterations - 1);
    let p95 = samples[p95_index];
    let min = samples[0];
    let max = samples[iterations - 1];

    println!(
        "{name:<40} mean {:>9}  p50 {:>9}  p95 {:>9}  min {:>9}  max {:>9}",
        fmt_duration(mean),
        fmt_duration(median),
        fmt_duration(p95),
        fmt_duration(min),
        fmt_duration(max),
    );
}

fn bench_command(name: &str, iterations: usize, cwd: Option<PathBuf>, args: &[&str]) {
    bench(name, iterations, || {
        let mut command = Command::new(args[0]);
        command.args(&args[1..]);

        if let Some(ref cwd) = cwd {
            command.current_dir(cwd);
        }

        match command.output() {
            Ok(output) => black_box((
                output.status.code(),
                output.stdout.len(),
                output.stderr.len(),
            )),
            Err(err) => black_box((None, 0, err.to_string().len())),
        }
    });
}

fn fmt_duration(duration: Duration) -> String {
    let micros = duration.as_secs_f64() * 1_000_000.0;
    if micros >= 1_000_000.0 {
        format!("{:.2}s", micros / 1_000_000.0)
    } else if micros >= 1_000.0 {
        format!("{:.2}ms", micros / 1_000.0)
    } else {
        format!("{micros:.0}us")
    }
}

fn init_git_repo(repo_root: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(repo_root)?;

    let status = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_root)
        .status()?;
    if !status.success() {
        return Err("git init failed".into());
    }

    Ok(())
}

fn make_raw_panes(count: usize) -> Vec<RawWezTermPane> {
    (0..count)
        .map(|idx| {
            let mut user_vars = std::collections::HashMap::new();
            user_vars.insert("agent_type".into(), "codex".into());
            user_vars.insert(
                "agent_status".into(),
                match idx % 4 {
                    0 => "running",
                    1 => "waiting",
                    2 => "idle",
                    _ => "error",
                }
                .into(),
            );
            user_vars.insert("agent_prompt".into(), format!("prompt-{idx}"));
            user_vars.insert("agent_cwd".into(), format!("/tmp/project-{idx}"));
            user_vars.insert("agent_started_at".into(), "1700000000".into());
            user_vars.insert("agent_subagents".into(), "worker,worker".into());

            RawWezTermPane {
                window_id: (idx / 6) as u64,
                tab_id: (idx / 3) as u64,
                tab_title: format!("tab-{idx}"),
                pane_id: idx as u64 + 1,
                workspace: format!("workspace-{}", idx % 4),
                title: String::new(),
                cwd: "file:///tmp".into(),
                is_active: idx == 0,
                is_zoomed: false,
                tty_name: String::new(),
                user_vars,
            }
        })
        .collect()
}

fn make_workspaces(
    repo_root: &Path,
    pane_count: usize,
) -> Result<Vec<WorkspaceInfo>, Box<dyn Error>> {
    let mut panes = Vec::with_capacity(pane_count);

    for idx in 0..pane_count {
        let path = repo_root.join(format!("pane-{idx}"));
        fs::create_dir_all(&path)?;
        panes.push(PaneInfo {
            pane_id: idx as u64 + 1,
            tab_id: (idx / 6) as u64 + 1,
            window_id: (idx / 6) as u64 + 1,
            workspace: "default".into(),
            pane_active: idx == 0,
            status: PaneStatus::Running,
            attention: false,
            agent: AgentType::Codex,
            path: path.to_string_lossy().into_owned(),
            prompt: "benchmark".into(),
            prompt_is_response: false,
            started_at: Some(1_700_000_000),
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: Vec::new(),
            usage: wezterm_agent_dashboard::usage::UsageStats::default(),
        });
    }

    let tabs = panes
        .chunks(6)
        .enumerate()
        .map(|(idx, panes)| TabInfo {
            tab_id: idx as u64 + 1,
            tab_title: format!("tab-{}", idx + 1),
            tab_active: idx == 0,
            panes: panes.to_vec(),
        })
        .collect();

    Ok(vec![WorkspaceInfo {
        workspace_name: "default".into(),
        tabs,
    }])
}

fn write_activity_logs(pane_ids: &[u64], line_count: usize) -> Result<(), Box<dyn Error>> {
    for (offset, pane_id) in pane_ids.iter().copied().enumerate() {
        let mut lines = Vec::with_capacity(line_count);
        for idx in 0..line_count {
            let task_id = idx % 20 + 1;
            let line = match idx % 3 {
                0 => format!(
                    "12:{:02}|TaskCreate|#{task_id} task-{offset}-{idx}",
                    idx % 60
                ),
                1 => format!("12:{:02}|TaskUpdate|in_progress #{task_id}", idx % 60),
                _ => format!("12:{:02}|TaskUpdate|completed #{task_id}", idx % 60),
            };
            lines.push(line);
        }
        fs::write(activity::log_file_path(pane_id), lines.join("\n"))?;
    }

    Ok(())
}

fn simulate_task_progress_refresh(pane_ids: &[u64]) -> usize {
    pane_ids
        .iter()
        .map(|pane_id| {
            let entries = activity::read_activity_log(*pane_id, 100);
            let progress: TaskProgress = activity::parse_task_progress(&entries);
            progress.total_count()
        })
        .sum()
}
