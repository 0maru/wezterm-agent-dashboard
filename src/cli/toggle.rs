use crate::wezterm;
use crate::wezterm::RawWezTermPane;

/// Toggle the dashboard sidebar pane.
/// If a dashboard pane exists in the current tab, kill it.
/// Otherwise, create a new one by splitting the current pane.
pub fn cmd_toggle(args: &[String]) -> i32 {
    let sidebar_percent: u8 = args.first().and_then(|s| s.parse().ok()).unwrap_or(20);

    let raw_panes = wezterm::query_all_panes();
    let current_pane_id = std::env::var("WEZTERM_PANE")
        .ok()
        .and_then(|s| s.parse::<u64>().ok());

    if let Some(dashboard_pane_id) = find_dashboard_in_current_tab(&raw_panes, current_pane_id) {
        wezterm::kill_pane(dashboard_pane_id);
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

fn find_dashboard_in_current_tab(
    raw_panes: &[RawWezTermPane],
    current_pane_id: Option<u64>,
) -> Option<u64> {
    let current_tab_id = current_pane_id
        .and_then(|pane_id| raw_panes.iter().find(|p| p.pane_id == pane_id))
        .map(|p| p.tab_id)?;

    raw_panes
        .iter()
        .find(|p| p.tab_id == current_tab_id && is_dashboard_pane(p))
        .map(|p| p.pane_id)
}

fn is_dashboard_pane(pane: &RawWezTermPane) -> bool {
    pane.user_vars
        .get("pane_role")
        .is_some_and(|r| r == "dashboard")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn pane(pane_id: u64, tab_id: u64, dashboard: bool) -> RawWezTermPane {
        let mut user_vars = HashMap::new();
        if dashboard {
            user_vars.insert("pane_role".to_string(), "dashboard".to_string());
        }

        RawWezTermPane {
            window_id: 1,
            tab_id,
            tab_title: format!("tab-{tab_id}"),
            pane_id,
            workspace: "default".to_string(),
            title: String::new(),
            cwd: String::new(),
            is_active: false,
            is_zoomed: false,
            tty_name: String::new(),
            user_vars,
        }
    }

    #[test]
    fn test_find_dashboard_in_current_tab() {
        let panes = vec![pane(1, 10, false), pane(2, 10, true), pane(3, 20, true)];

        assert_eq!(find_dashboard_in_current_tab(&panes, Some(1)), Some(2));
    }

    #[test]
    fn test_find_dashboard_ignores_other_tabs() {
        let panes = vec![pane(1, 10, false), pane(2, 20, true)];

        assert_eq!(find_dashboard_in_current_tab(&panes, Some(1)), None);
    }

    #[test]
    fn test_find_dashboard_without_current_pane() {
        let panes = vec![pane(2, 20, true)];

        assert_eq!(find_dashboard_in_current_tab(&panes, None), None);
        assert_eq!(find_dashboard_in_current_tab(&panes, Some(999)), None);
    }
}
