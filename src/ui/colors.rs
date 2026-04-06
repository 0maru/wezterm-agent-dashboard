use ratatui::style::Color;

/// Color theme for the dashboard UI.
/// Uses 256-color indices matching tmux-agent-sidebar's defaults.
#[derive(Debug, Clone)]
pub struct ColorTheme {
    pub running: Color,
    pub waiting: Color,
    pub idle: Color,
    pub error: Color,
    pub claude: Color,
    pub codex: Color,
    pub active_border: Color,
    pub inactive_border: Color,
    pub dimmed: Color,
    pub filter_active: Color,
    pub filter_inactive: Color,
    pub prompt_text: Color,
    pub elapsed_time: Color,
    pub badge_plan: Color,
    pub badge_edit: Color,
    pub badge_auto: Color,
    pub badge_bypass: Color,
    pub subagent: Color,
    pub wait_reason: Color,
    pub branch: Color,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            running: Color::Indexed(114),    // green
            waiting: Color::Indexed(221),    // yellow
            idle: Color::Indexed(109),       // teal
            error: Color::Indexed(203),      // red
            claude: Color::Indexed(174),     // terracotta
            codex: Color::Indexed(141),      // purple
            active_border: Color::Indexed(117),   // cyan
            inactive_border: Color::Indexed(240), // dark gray
            dimmed: Color::Indexed(245),     // medium gray
            filter_active: Color::White,
            filter_inactive: Color::Indexed(245),
            prompt_text: Color::Indexed(252),
            elapsed_time: Color::Indexed(245),
            badge_plan: Color::Indexed(117),   // blue
            badge_edit: Color::Indexed(180),   // soft yellow
            badge_auto: Color::Indexed(221),   // yellow
            badge_bypass: Color::Indexed(203), // red
            subagent: Color::Indexed(245),
            wait_reason: Color::Indexed(221),  // yellow
            branch: Color::Indexed(117),       // cyan
        }
    }
}

impl ColorTheme {
    /// Get the color for a pane status.
    pub fn status_color(&self, status: crate::wezterm::PaneStatus) -> Color {
        match status {
            crate::wezterm::PaneStatus::Running => self.running,
            crate::wezterm::PaneStatus::Waiting => self.waiting,
            crate::wezterm::PaneStatus::Idle => self.idle,
            crate::wezterm::PaneStatus::Error => self.error,
            crate::wezterm::PaneStatus::Unknown => self.dimmed,
        }
    }

    /// Get the color for an agent type label.
    pub fn agent_color(&self, agent: crate::wezterm::AgentType) -> Color {
        match agent {
            crate::wezterm::AgentType::Claude => self.claude,
            crate::wezterm::AgentType::Codex => self.codex,
        }
    }

    /// Get the color for a permission mode badge.
    pub fn badge_color(&self, mode: crate::wezterm::PermissionMode) -> Color {
        match mode {
            crate::wezterm::PermissionMode::Plan => self.badge_plan,
            crate::wezterm::PermissionMode::AcceptEdits => self.badge_edit,
            crate::wezterm::PermissionMode::Auto => self.badge_auto,
            crate::wezterm::PermissionMode::BypassPermissions => self.badge_bypass,
            crate::wezterm::PermissionMode::Default => self.dimmed,
        }
    }
}
