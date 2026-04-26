use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use wezterm_agent_dashboard::git;
use wezterm_agent_dashboard::group;
use wezterm_agent_dashboard::state::{AppState, BottomTab, Focus, RefreshActions, RepoFilter};
use wezterm_agent_dashboard::ui::Dashboard;
use wezterm_agent_dashboard::user_vars;

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const SPINNER_INTERVAL: Duration = Duration::from_millis(200);

fn main() -> io::Result<()> {
    // Check for CLI subcommands
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = wezterm_agent_dashboard::cli::run(&args) {
        std::process::exit(code);
    }

    // Get our pane ID from WEZTERM_PANE
    let wezterm_pane: u64 = match std::env::var("WEZTERM_PANE")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        Some(id) => id,
        None => {
            eprintln!("Error: WEZTERM_PANE environment variable not set.");
            eprintln!("This binary must be run inside a WezTerm pane.");
            std::process::exit(1);
        }
    };

    // Mark ourselves as the dashboard pane
    user_vars::set_user_var("pane_role", "dashboard");

    // Enter alternate screen + raw mode
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(
        stdout,
        EnterAlternateScreen,
        cursor::Hide,
        event::EnableMouseCapture
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, wezterm_pane);

    // Cleanup
    terminal::disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        cursor::Show,
        event::DisableMouseCapture
    )?;

    // Clear dashboard pane role
    user_vars::clear_user_var("pane_role");

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    dashboard_pane_id: u64,
) -> io::Result<()> {
    let mut state = AppState::new(dashboard_pane_id);

    // Start repo polling thread
    let (repo_rx, repo_path_tx, repo_shutdown, _repo_handle) = group::start_repo_poll_thread();

    // Start git polling thread
    let git_active = Arc::new(AtomicBool::new(false));
    let (git_rx, git_path_tx, git_shutdown, _git_handle) =
        git::start_git_poll_thread(git_active.clone());

    // Initial refresh
    dispatch_refresh_actions(state.refresh(), &repo_path_tx, &git_path_tx);

    let mut last_refresh = Instant::now();
    let mut last_spinner = Instant::now();

    loop {
        // Draw
        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(Dashboard { state: &state }, area);
        })?;

        // Calculate poll timeout
        let since_refresh = last_refresh.elapsed();
        let since_spinner = last_spinner.elapsed();
        let refresh_remaining = REFRESH_INTERVAL.saturating_sub(since_refresh);
        let spinner_remaining = SPINNER_INTERVAL.saturating_sub(since_spinner);
        let timeout = refresh_remaining.min(spinner_remaining);

        // Poll for events
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    match (key.code, key.modifiers) {
                        // Quit
                        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            repo_shutdown.store(true, Ordering::Relaxed);
                            git_shutdown.store(true, Ordering::Relaxed);
                            break;
                        }

                        // Navigation: focus zones
                        (KeyCode::Tab, KeyModifiers::NONE) => {
                            state.focus = match state.focus {
                                Focus::Filter => Focus::Agents,
                                Focus::Agents => Focus::ActivityLog,
                                Focus::ActivityLog => Focus::Filter,
                            };
                        }

                        // Bottom tab toggle
                        (KeyCode::BackTab, _) => {
                            state.bottom_tab = state.bottom_tab.toggle();
                            git_active
                                .store(state.bottom_tab == BottomTab::GitStatus, Ordering::Relaxed);
                        }

                        // Filter navigation (when filter is focused)
                        (KeyCode::Char('h') | KeyCode::Left, _) if state.focus == Focus::Filter => {
                            state.agent_filter = state.agent_filter.prev();
                            send_git_path(state.refresh_local_views(), &git_path_tx);
                        }
                        (KeyCode::Char('l') | KeyCode::Right, _)
                            if state.focus == Focus::Filter =>
                        {
                            state.agent_filter = state.agent_filter.next();
                            send_git_path(state.refresh_local_views(), &git_path_tx);
                        }

                        // Popup navigation
                        (KeyCode::Char('j') | KeyCode::Down, _) if state.repo_popup_open => {
                            let max = state.repo_entries().len();
                            if state.repo_popup_selected < max {
                                state.repo_popup_selected += 1;
                            }
                        }
                        (KeyCode::Char('k') | KeyCode::Up, _)
                            if state.repo_popup_open && state.repo_popup_selected > 0 =>
                        {
                            state.repo_popup_selected -= 1;
                        }

                        // Agent list navigation
                        (KeyCode::Char('j') | KeyCode::Down, _) if state.focus == Focus::Agents => {
                            state.select_next();
                        }
                        (KeyCode::Char('k') | KeyCode::Up, _) if state.focus == Focus::Agents => {
                            state.select_prev();
                        }

                        // Jump to pane
                        (KeyCode::Enter, _) if state.focus == Focus::Agents => {
                            if state.repo_popup_open {
                                // Select repo filter
                                let entries = state.repo_entries();
                                if state.repo_popup_selected == 0 {
                                    state.repo_filter = RepoFilter::All;
                                } else if let Some((id, _name)) =
                                    entries.get(state.repo_popup_selected - 1)
                                {
                                    state.repo_filter = RepoFilter::Repo(id.clone());
                                }
                                state.repo_popup_open = false;
                                send_git_path(state.refresh_local_views(), &git_path_tx);
                            } else {
                                state.jump_to_selected();
                            }
                        }

                        // Repo filter popup
                        (KeyCode::Char('r'), _) if state.focus == Focus::Agents => {
                            state.repo_popup_open = !state.repo_popup_open;
                            state.repo_popup_selected = 0;
                        }

                        // Escape closes popup or clears filter
                        (KeyCode::Esc, _) => {
                            if state.repo_popup_open {
                                state.repo_popup_open = false;
                            } else if state.repo_filter != RepoFilter::All {
                                state.repo_filter = RepoFilter::All;
                                send_git_path(state.refresh_local_views(), &git_path_tx);
                            }
                        }

                        _ => {}
                    }
                }

                Event::Mouse(mouse) => {
                    if let MouseEventKind::Down(_) = mouse.kind
                        && state.repo_popup_open
                    {
                        state.repo_popup_open = false;
                    }
                }

                Event::Resize(_, _) => {
                    // Terminal resized, just redraw
                }

                _ => {}
            }
        }

        // Spinner update
        if last_spinner.elapsed() >= SPINNER_INTERVAL {
            state.spinner_frame = state.spinner_frame.wrapping_add(1);
            last_spinner = Instant::now();
        }

        // Periodic refresh
        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            dispatch_refresh_actions(state.refresh(), &repo_path_tx, &git_path_tx);
            last_refresh = Instant::now();
        }

        // Check for repo data from background thread
        let mut repo_updates = Vec::new();
        while let Ok(update) = repo_rx.try_recv() {
            repo_updates.push(update);
        }
        if !repo_updates.is_empty() {
            dispatch_refresh_actions(
                state.apply_repo_info_updates(repo_updates),
                &repo_path_tx,
                &git_path_tx,
            );
        }

        // Check for git data from background thread
        let mut latest_git = None;
        while let Ok(data) = git_rx.try_recv() {
            latest_git = Some(data);
        }
        if let Some(data) = latest_git {
            state.git = data;
        }
    }

    Ok(())
}

fn dispatch_refresh_actions(
    actions: RefreshActions,
    repo_path_tx: &Sender<String>,
    git_path_tx: &Sender<String>,
) {
    for path in actions.repo_paths {
        let _ = repo_path_tx.send(path);
    }

    send_git_path(actions.git_path, git_path_tx);
}

fn send_git_path(path: Option<String>, git_path_tx: &Sender<String>) {
    if let Some(path) = path {
        let _ = git_path_tx.send(path);
    }
}
