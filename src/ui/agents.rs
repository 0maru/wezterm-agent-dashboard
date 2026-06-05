use crate::SPINNER_ICON;
use crate::SPINNER_PULSE;
use crate::state::{AgentFilter, AppState, Focus, RepoFilter};
use crate::ui::text;
use crate::usage;
use crate::wezterm::PaneStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

// ---------------------------------------------------------------------------
// Filter bar
// ---------------------------------------------------------------------------

pub struct FilterBar<'a> {
    pub state: &'a AppState,
}

impl Widget for FilterBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let (all, running, waiting, idle, error) = self.state.status_counts();
        let filter = &self.state.ui.filters.agent_filter;
        let focused = self.state.ui.focus == Focus::Filter;

        let items: Vec<(AgentFilter, &str, usize, Color)> = vec![
            (AgentFilter::All, "All", all, Color::White),
            (
                AgentFilter::Running,
                PaneStatus::Running.icon(),
                running,
                self.state.ui.theme.running,
            ),
            (
                AgentFilter::Waiting,
                PaneStatus::Waiting.icon(),
                waiting,
                self.state.ui.theme.waiting,
            ),
            (
                AgentFilter::Idle,
                PaneStatus::Idle.icon(),
                idle,
                self.state.ui.theme.idle,
            ),
            (
                AgentFilter::Error,
                PaneStatus::Error.icon(),
                error,
                self.state.ui.theme.error,
            ),
        ];

        let mut spans = Vec::new();
        for (i, (af, label, count, color)) in items.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }

            let is_active = filter == af;
            let style = if is_active && focused {
                Style::default()
                    .fg(*color)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else if is_active {
                Style::default().fg(*color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.state.ui.theme.filter_inactive)
            };

            spans.push(Span::styled(format!("{label}{count}"), style));
        }

        // Repo filter button
        if let RepoFilter::Repo(ref id) = self.state.ui.filters.repo_filter {
            let display = self
                .state
                .repos
                .groups
                .iter()
                .find(|g| g.id == *id)
                .map(|g| g.name.as_str())
                .unwrap_or(id.as_str());
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("▼{display}"),
                Style::default().fg(self.state.ui.theme.active_border),
            ));
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

// ---------------------------------------------------------------------------
// Agent list
// ---------------------------------------------------------------------------

pub struct AgentList<'a> {
    pub state: &'a AppState,
}

impl Widget for AgentList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let width = area.width as usize;
        let mut y = area.y;
        let max_y = area.y + area.height;

        for (group_idx, group) in self.state.repos.groups.iter().enumerate() {
            if let RepoFilter::Repo(ref id) = self.state.ui.filters.repo_filter
                && group.id != *id
            {
                continue;
            }

            // Group header
            if y < max_y && self.state.repos.groups.len() > 1 {
                let border_color = if group.has_focus {
                    self.state.ui.theme.active_border
                } else {
                    self.state.ui.theme.inactive_border
                };

                let header = format!("─ {} ", group.name);
                let header = text::truncate(&header, width);
                let remaining =
                    width.saturating_sub(unicode_width::UnicodeWidthStr::width(header.as_str()));
                let line_str = format!("{header}{}", "─".repeat(remaining));

                buf.set_string(area.x, y, &line_str, Style::default().fg(border_color));
                y += 1;
            }

            // Panes in this group
            for (pane, git_info) in &group.panes {
                if !self.state.ui.filters.agent_filter.matches(pane.status) {
                    continue;
                }

                if y >= max_y {
                    break;
                }

                let is_selected = self
                    .state
                    .agents
                    .row_targets
                    .get(self.state.agents.selected_row)
                    .is_some_and(|t| t.pane_id == pane.pane_id);

                // Line 1: status icon + agent type + permission badge + elapsed time
                let mut spans = Vec::new();

                // Status icon
                let status_color = self.state.ui.theme.status_color(pane.status);
                let icon = if pane.status == PaneStatus::Running {
                    let color_idx =
                        SPINNER_PULSE[self.state.ui.spinner_frame % SPINNER_PULSE.len()];
                    spans.push(Span::styled(
                        SPINNER_ICON,
                        Style::default().fg(Color::Indexed(color_idx)),
                    ));
                    // Don't push icon again
                    ""
                } else {
                    pane.status.icon()
                };

                if !icon.is_empty() {
                    spans.push(Span::styled(icon, Style::default().fg(status_color)));
                }

                spans.push(Span::raw(" "));

                // Agent type
                let agent_color = self.state.ui.theme.agent_color(pane.agent);
                spans.push(Span::styled(
                    pane.agent.as_str(),
                    Style::default()
                        .fg(agent_color)
                        .add_modifier(Modifier::BOLD),
                ));

                // Permission badge
                if let Some(badge) = pane.permission_mode.badge() {
                    let badge_color = self.state.ui.theme.badge_color(pane.permission_mode);
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(badge, Style::default().fg(badge_color)));
                }

                // Task progress
                if let Some(progress) = self.state.activity.pane_task_progress.get(&pane.pane_id)
                    && !progress.is_empty()
                {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        progress.display(),
                        Style::default().fg(self.state.ui.theme.dimmed),
                    ));
                }

                // トークン使用量
                if !pane.usage.is_empty() {
                    let mut usage_parts = Vec::new();
                    if pane.usage.total_tokens() > 0 {
                        usage_parts.push(format!(
                            "tok {}",
                            text::format_token_count(pane.usage.total_tokens())
                        ));
                    }
                    if let Some(cost) = pane.usage.cost_usd {
                        usage_parts.push(text::format_cost_usd(cost));
                    }
                    if !pane.usage.model.is_empty() {
                        usage_parts.push(usage::compact_model_name(&pane.usage.model));
                    }

                    if !usage_parts.is_empty() {
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(
                            usage_parts.join(" "),
                            Style::default().fg(self.state.theme.dimmed),
                        ));
                    }
                }

                // Elapsed time
                if let Some(started) = pane.started_at {
                    let elapsed = text::format_elapsed(self.state.now, started);
                    if !elapsed.is_empty() {
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(
                            elapsed,
                            Style::default().fg(self.state.ui.theme.elapsed_time),
                        ));
                    }
                }

                // Selection indicator
                if is_selected && self.state.ui.focus == Focus::Agents {
                    // Highlight the entire line
                    let line = Line::from(spans);
                    buf.set_line(area.x, y, &line, area.width);
                    // Add a subtle background highlight
                    for x in area.x..area.x + area.width {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
                        }
                    }
                } else {
                    let line = Line::from(spans);
                    buf.set_line(area.x, y, &line, area.width);
                }
                y += 1;

                // Line 2: prompt text (if present)
                if !pane.prompt.is_empty() && y < max_y {
                    let prefix = if pane.prompt_is_response { "▶ " } else { "" };
                    let prompt_text = format!("  {prefix}{}", pane.prompt);
                    let truncated = text::truncate(&prompt_text, width);

                    buf.set_string(
                        area.x,
                        y,
                        &truncated,
                        Style::default().fg(self.state.ui.theme.prompt_text),
                    );
                    y += 1;
                }

                // Wait reason
                if !pane.wait_reason.is_empty() && y < max_y {
                    let reason_text = format!("  ⏳ {}", pane.wait_reason);
                    let truncated = text::truncate(&reason_text, width);

                    buf.set_string(
                        area.x,
                        y,
                        &truncated,
                        Style::default().fg(self.state.ui.theme.wait_reason),
                    );
                    y += 1;
                }

                // Subagent tree
                if !pane.subagents.is_empty() && y < max_y {
                    for (i, sub) in pane.subagents.iter().enumerate() {
                        if y >= max_y {
                            break;
                        }
                        let connector = if i == pane.subagents.len() - 1 {
                            "  └─ "
                        } else {
                            "  ├─ "
                        };
                        let sub_text = format!("{connector}{sub}");
                        let truncated = text::truncate(&sub_text, width);

                        buf.set_string(
                            area.x,
                            y,
                            &truncated,
                            Style::default().fg(self.state.ui.theme.subagent),
                        );
                        y += 1;
                    }
                }

                // Git branch (if available)
                if let Some(branch) = &git_info.branch
                    && y < max_y
                {
                    let branch_text = if git_info.is_worktree {
                        format!("  🌿 {branch} (worktree)")
                    } else {
                        format!("  🌿 {branch}")
                    };
                    let truncated = text::truncate(&branch_text, width);

                    buf.set_string(
                        area.x,
                        y,
                        &truncated,
                        Style::default().fg(self.state.ui.theme.branch),
                    );
                    y += 1;
                }
            }

            // Spacing between groups
            if group_idx + 1 < self.state.repos.groups.len() && y < max_y {
                y += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Repo filter popup
// ---------------------------------------------------------------------------

pub struct RepoPopup<'a> {
    pub state: &'a AppState,
}

impl Widget for RepoPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Clear background
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(' ');
                    cell.set_style(Style::default());
                }
            }
        }

        // Border
        let border_style = Style::default().fg(self.state.ui.theme.active_border);
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut((x, area.y)) {
                cell.set_char('─');
                cell.set_style(border_style);
            }
            if let Some(cell) = buf.cell_mut((x, area.y + area.height - 1)) {
                cell.set_char('─');
                cell.set_style(border_style);
            }
        }

        let entries = self.state.repo_entries();
        let mut y = area.y + 1;

        // "All" option
        if y < area.y + area.height - 1 {
            let is_selected = self.state.ui.filters.repo_popup_selected == 0;
            let style = if is_selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            buf.set_string(area.x + 1, y, "All", style);
            y += 1;
        }

        // Repo entries
        for (i, (_id, name)) in entries.iter().enumerate() {
            if y >= area.y + area.height - 1 {
                break;
            }
            let is_selected = self.state.ui.filters.repo_popup_selected == i + 1;
            let style = if is_selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let display = text::truncate(name, (area.width - 2) as usize);
            buf.set_string(area.x + 1, y, &display, style);
            y += 1;
        }
    }
}
