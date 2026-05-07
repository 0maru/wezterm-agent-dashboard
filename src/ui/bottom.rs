use crate::state::{AppState, BottomTab};
use crate::ui::text;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

/// Height of the bottom panel (including tab label).
pub const BOTTOM_PANEL_HEIGHT: u16 = 12;

// ---------------------------------------------------------------------------
// Bottom tab label
// ---------------------------------------------------------------------------

pub struct BottomTabLabel<'a> {
    pub state: &'a AppState,
}

impl Widget for BottomTabLabel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let tabs = [
            (BottomTab::Activity, "Activity"),
            (BottomTab::GitStatus, "Git"),
        ];

        let mut spans = Vec::new();
        spans.push(Span::raw(" "));

        for (i, (tab, label)) in tabs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(
                    " │ ",
                    Style::default().fg(self.state.ui.theme.inactive_border),
                ));
            }

            let is_active = self.state.ui.bottom_tab == *tab;
            let style = if is_active {
                Style::default()
                    .fg(self.state.ui.theme.filter_active)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.state.ui.theme.filter_inactive)
            };

            spans.push(Span::styled(*label, style));
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

// ---------------------------------------------------------------------------
// Activity panel
// ---------------------------------------------------------------------------

pub struct ActivityPanel<'a> {
    pub state: &'a AppState,
}

impl Widget for ActivityPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let width = area.width as usize;

        if self.state.activity.entries.is_empty() {
            buf.set_string(
                area.x + 1,
                area.y,
                "No activity yet",
                Style::default().fg(self.state.ui.theme.dimmed),
            );
            return;
        }

        for (i, entry) in self.state.activity.entries.iter().enumerate() {
            if i >= area.height as usize {
                break;
            }

            let y = area.y + i as u16;

            // Timestamp
            let ts_style = Style::default().fg(self.state.ui.theme.dimmed);
            buf.set_string(area.x + 1, y, &entry.timestamp, ts_style);

            // Tool name with color
            let tool_color = Color::Indexed(entry.tool_color_index());
            let tool_style = Style::default().fg(tool_color);
            let tool_x = area.x + 7; // after "HH:MM "
            buf.set_string(tool_x, y, &entry.tool, tool_style);

            // Label
            let label_x = tool_x + entry.tool.len() as u16 + 1;
            let remaining = width.saturating_sub(label_x as usize - area.x as usize);
            if remaining > 0 && !entry.label.is_empty() {
                let label = text::truncate(&entry.label, remaining);
                buf.set_string(
                    label_x,
                    y,
                    &label,
                    Style::default().fg(self.state.ui.theme.prompt_text),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Git panel
// ---------------------------------------------------------------------------

pub struct GitPanel<'a> {
    pub state: &'a AppState,
}

impl Widget for GitPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let git = &self.state.repos.git;
        let width = area.width as usize;
        let mut y = area.y;
        let max_y = area.y + area.height;

        if git.branch.is_empty() {
            buf.set_string(
                area.x + 1,
                y,
                "No git info",
                Style::default().fg(self.state.ui.theme.dimmed),
            );
            return;
        }

        // Branch + ahead/behind
        if y < max_y {
            let mut spans = vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    &git.branch,
                    Style::default()
                        .fg(self.state.ui.theme.branch)
                        .add_modifier(Modifier::BOLD),
                ),
            ];

            if git.ahead > 0 || git.behind > 0 {
                spans.push(Span::raw("  "));
                if git.ahead > 0 {
                    spans.push(Span::styled(
                        format!("↑{}", git.ahead),
                        Style::default().fg(self.state.ui.theme.running),
                    ));
                }
                if git.behind > 0 {
                    if git.ahead > 0 {
                        spans.push(Span::raw(" "));
                    }
                    spans.push(Span::styled(
                        format!("↓{}", git.behind),
                        Style::default().fg(self.state.ui.theme.error),
                    ));
                }
            }

            if let Some(pr) = git.pr_number {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    format!("PR #{pr}"),
                    Style::default().fg(self.state.ui.theme.active_border),
                ));
            }

            let line = Line::from(spans);
            buf.set_line(area.x, y, &line, area.width);
            y += 1;
        }

        // Staged files
        if !git.staged_files.is_empty() && y < max_y {
            buf.set_string(
                area.x + 1,
                y,
                format!(
                    "Staged ({}) +{} -{}",
                    git.staged_files.len(),
                    git.staged_insertions,
                    git.staged_deletions
                ),
                Style::default().fg(self.state.ui.theme.running),
            );
            y += 1;

            for file in &git.staged_files {
                if y >= max_y {
                    break;
                }
                let line = format!("  {} +{} -{}", file.file, file.insertions, file.deletions);
                let truncated = text::truncate(&line, width - 1);
                buf.set_string(
                    area.x + 1,
                    y,
                    &truncated,
                    Style::default().fg(self.state.ui.theme.prompt_text),
                );
                y += 1;
            }
        }

        // Unstaged files
        if !git.unstaged_files.is_empty() && y < max_y {
            buf.set_string(
                area.x + 1,
                y,
                format!(
                    "Unstaged ({}) +{} -{}",
                    git.unstaged_files.len(),
                    git.unstaged_insertions,
                    git.unstaged_deletions
                ),
                Style::default().fg(self.state.ui.theme.waiting),
            );
            y += 1;

            for file in &git.unstaged_files {
                if y >= max_y {
                    break;
                }
                let line = format!("  {} +{} -{}", file.file, file.insertions, file.deletions);
                let truncated = text::truncate(&line, width - 1);
                buf.set_string(
                    area.x + 1,
                    y,
                    &truncated,
                    Style::default().fg(self.state.ui.theme.prompt_text),
                );
                y += 1;
            }
        }

        // Untracked files
        if !git.untracked_files.is_empty() && y < max_y {
            buf.set_string(
                area.x + 1,
                y,
                format!("Untracked ({})", git.untracked_files.len()),
                Style::default().fg(self.state.ui.theme.dimmed),
            );
            y += 1;

            for file in &git.untracked_files {
                if y >= max_y {
                    break;
                }
                let truncated = text::truncate(&format!("  {file}"), width - 1);
                buf.set_string(
                    area.x + 1,
                    y,
                    &truncated,
                    Style::default().fg(self.state.ui.theme.dimmed),
                );
                y += 1;
            }
        }

        // Remote URL
        if !git.remote_url.is_empty() && y < max_y {
            let truncated = text::truncate(&format!("  {}", git.remote_url), width - 1);
            buf.set_string(
                area.x + 1,
                y,
                &truncated,
                Style::default().fg(self.state.ui.theme.dimmed),
            );
        }
    }
}
