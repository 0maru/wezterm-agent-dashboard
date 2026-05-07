use crate::wezterm::{self, RawWezTermPane, SplitDirection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ToggleOptions {
    sidebar_percent: u8,
    direction: SplitDirection,
}

impl Default for ToggleOptions {
    fn default() -> Self {
        Self {
            sidebar_percent: 20,
            direction: SplitDirection::Right,
        }
    }
}

/// Toggle the dashboard sidebar pane.
/// If a dashboard pane exists in the current tab, kill it.
/// Otherwise, create a new one by splitting the current pane.
pub fn cmd_toggle(args: &[String]) -> i32 {
    let options = match parse_toggle_options(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };

    let raw_panes = wezterm::query_all_panes();
    let current_pane_id = std::env::var("WEZTERM_PANE")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let current_tab_id = find_current_tab_id(&raw_panes, current_pane_id);

    let existing_dashboard =
        current_tab_id.and_then(|tab_id| find_dashboard_pane_in_tab(&raw_panes, tab_id));

    if let Some(dashboard) = existing_dashboard {
        wezterm::kill_pane(dashboard.pane_id);
        return 0;
    }

    let self_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "wezterm-agent-dashboard".to_string());

    match wezterm::split_pane(options.sidebar_percent, options.direction, &[&self_bin]) {
        Some(_pane_id) => 0,
        None => {
            eprintln!("Failed to create dashboard pane");
            1
        }
    }
}

fn parse_toggle_options(args: &[String]) -> Result<ToggleOptions, String> {
    let mut options = ToggleOptions::default();
    let mut idx = 0;

    while idx < args.len() {
        match args[idx].as_str() {
            "--percent" | "-p" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "Missing value for --percent".to_string())?;
                options.sidebar_percent = parse_percent(value)?;
            }
            "--position" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "Missing value for --position".to_string())?;
                options.direction = parse_direction(value)?;
            }
            "--left" => options.direction = SplitDirection::Left,
            "--right" => options.direction = SplitDirection::Right,
            "--top" => options.direction = SplitDirection::Top,
            "--bottom" => options.direction = SplitDirection::Bottom,
            value if idx == 0 && !value.starts_with('-') => {
                options.sidebar_percent = parse_percent(value)?;
            }
            value => return Err(format!("Unknown toggle option: {value}")),
        }

        idx += 1;
    }

    Ok(options)
}

fn parse_percent(value: &str) -> Result<u8, String> {
    let percent = value
        .parse::<u8>()
        .map_err(|_| format!("Invalid sidebar percent: {value}"))?;
    if (1..=100).contains(&percent) {
        Ok(percent)
    } else {
        Err(format!("Sidebar percent must be 1..=100: {value}"))
    }
}

fn parse_direction(value: &str) -> Result<SplitDirection, String> {
    SplitDirection::from_str(value).ok_or_else(|| {
        format!("Invalid sidebar position: {value} (expected Left, Right, Top, or Bottom)")
    })
}

fn find_current_tab_id(raw_panes: &[RawWezTermPane], current_pane_id: Option<u64>) -> Option<u64> {
    if let Some(pane_id) = current_pane_id
        && let Some(pane) = raw_panes.iter().find(|pane| pane.pane_id == pane_id)
    {
        return Some(pane.tab_id);
    }

    raw_panes
        .iter()
        .find(|pane| pane.is_active)
        .map(|pane| pane.tab_id)
}

fn find_dashboard_pane_in_tab(
    raw_panes: &[RawWezTermPane],
    tab_id: u64,
) -> Option<&RawWezTermPane> {
    raw_panes.iter().find(|pane| {
        pane.tab_id == tab_id
            && pane
                .user_vars
                .get("pane_role")
                .is_some_and(|role| role == "dashboard")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_parse_toggle_options_defaults() {
        let options = parse_toggle_options(&[]).unwrap();

        assert_eq!(options.sidebar_percent, 20);
        assert_eq!(options.direction, SplitDirection::Right);
    }

    #[test]
    fn test_parse_toggle_options_legacy_percent() {
        let options = parse_toggle_options(&["30".to_string()]).unwrap();

        assert_eq!(options.sidebar_percent, 30);
        assert_eq!(options.direction, SplitDirection::Right);
    }

    #[test]
    fn test_parse_toggle_options_percent_and_position() {
        let options = parse_toggle_options(&[
            "--percent".to_string(),
            "25".to_string(),
            "--position".to_string(),
            "Left".to_string(),
        ])
        .unwrap();

        assert_eq!(options.sidebar_percent, 25);
        assert_eq!(options.direction, SplitDirection::Left);
    }

    #[test]
    fn test_parse_toggle_options_direction_flag() {
        let options = parse_toggle_options(&["--bottom".to_string()]).unwrap();

        assert_eq!(options.direction, SplitDirection::Bottom);
    }

    #[test]
    fn test_parse_toggle_options_rejects_invalid_percent() {
        assert!(parse_toggle_options(&["0".to_string()]).is_err());
        assert!(parse_toggle_options(&["101".to_string()]).is_err());
    }

    #[test]
    fn test_find_current_tab_id_prefers_env_pane() {
        let panes = vec![
            raw_pane(1, 10, true, false),
            raw_pane(2, 20, false, false),
            raw_pane(3, 20, false, true),
        ];

        assert_eq!(find_current_tab_id(&panes, Some(3)), Some(20));
    }

    #[test]
    fn test_find_current_tab_id_falls_back_to_active_pane() {
        let panes = vec![raw_pane(1, 10, false, false), raw_pane(2, 20, true, false)];

        assert_eq!(find_current_tab_id(&panes, None), Some(20));
    }

    #[test]
    fn test_find_dashboard_pane_in_tab_ignores_other_tabs() {
        let panes = vec![
            raw_pane(1, 10, false, true),
            raw_pane(2, 20, false, false),
            raw_pane(3, 20, false, true),
        ];

        let dashboard = find_dashboard_pane_in_tab(&panes, 20).unwrap();

        assert_eq!(dashboard.pane_id, 3);
    }

    fn raw_pane(pane_id: u64, tab_id: u64, is_active: bool, is_dashboard: bool) -> RawWezTermPane {
        let mut user_vars = HashMap::new();
        if is_dashboard {
            user_vars.insert("pane_role".to_string(), "dashboard".to_string());
        }

        RawWezTermPane {
            window_id: 1,
            tab_id,
            tab_title: "tab".to_string(),
            pane_id,
            workspace: "default".to_string(),
            title: String::new(),
            cwd: String::new(),
            is_active,
            is_zoomed: false,
            tty_name: String::new(),
            user_vars,
        }
    }
}
