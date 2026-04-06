pub mod agents;
pub mod bottom;
pub mod colors;
pub mod text;

use crate::state::{AppState, BottomTab};
use bottom::BOTTOM_PANEL_HEIGHT;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Widget;

/// Main dashboard layout.
///
/// ```text
/// ┌─────────────────────────┐
/// │  Filter bar (1 row)     │
/// ├─────────────────────────┤
/// │                         │
/// │  Agent list (flex)      │
/// │                         │
/// ├─────────────────────────┤
/// │  Bottom tab label (1)   │
/// ├─────────────────────────┤
/// │                         │
/// │  Bottom panel (10)      │
/// │                         │
/// └─────────────────────────┘
/// ```
pub struct Dashboard<'a> {
    pub state: &'a AppState,
}

impl Widget for Dashboard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 || area.width < 10 {
            return;
        }

        let bottom_height = BOTTOM_PANEL_HEIGHT.min(area.height / 2);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),                         // filter bar
                Constraint::Min(3),                           // agent list
                Constraint::Length(1),                         // tab label
                Constraint::Length(bottom_height.saturating_sub(1)), // bottom panel
            ])
            .split(area);

        // Filter bar
        agents::FilterBar { state: self.state }.render(chunks[0], buf);

        // Agent list
        agents::AgentList { state: self.state }.render(chunks[1], buf);

        // Separator line
        for x in chunks[2].x..chunks[2].x + chunks[2].width {
            if let Some(cell) = buf.cell_mut((x, chunks[2].y)) {
                cell.set_char('─');
                cell.set_style(Style::default().fg(self.state.theme.inactive_border));
            }
        }

        // Bottom tab label (overlay on separator)
        bottom::BottomTabLabel { state: self.state }.render(chunks[2], buf);

        // Bottom panel
        match self.state.bottom_tab {
            BottomTab::Activity => {
                bottom::ActivityPanel { state: self.state }.render(chunks[3], buf);
            }
            BottomTab::GitStatus => {
                bottom::GitPanel { state: self.state }.render(chunks[3], buf);
            }
        }

        // Repo filter popup (if open)
        if self.state.repo_popup_open {
            let popup_width = 30.min(area.width - 2);
            let names = self.state.repo_names();
            let popup_height = (names.len() as u16 + 3).min(area.height - 2);

            let popup_area = Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: popup_width,
                height: popup_height,
            };

            agents::RepoPopup { state: self.state }.render(popup_area, buf);
        }
    }
}
