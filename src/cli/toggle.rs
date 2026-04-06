use crate::wezterm;

/// Toggle the dashboard sidebar pane.
/// If a dashboard pane exists in the current tab, kill it.
/// Otherwise, create a new one by splitting the current pane.
pub fn cmd_toggle(args: &[String]) -> i32 {
    let sidebar_percent: u8 = args
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    let raw_panes = wezterm::query_all_panes();

    // Find an existing dashboard pane (any pane with pane_role = "dashboard")
    let existing_dashboard = raw_panes.iter().find(|p| {
        p.user_vars
            .get("pane_role")
            .is_some_and(|r| r == "dashboard")
    });

    if let Some(dashboard) = existing_dashboard {
        // Dashboard exists, kill it
        wezterm::kill_pane(dashboard.pane_id);
        return 0;
    }

    // Dashboard doesn't exist, create it
    let self_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "wezterm-agent-dashboard".to_string());

    match wezterm::split_pane_right(sidebar_percent, &[&self_bin]) {
        Some(_pane_id) => {
            // The new pane will run the TUI binary, which sets pane_role=dashboard on startup
            0
        }
        None => {
            eprintln!("Failed to create dashboard pane");
            1
        }
    }
}
