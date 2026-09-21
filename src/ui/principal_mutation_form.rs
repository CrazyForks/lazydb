use crate::{
    model::principal::PrincipalMutationForm,
    ui::{HitRegion, HitTarget, UiState, centered, theme::Theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    form: &PrincipalMutationForm,
    state: &mut UiState,
    theme: Theme,
) {
    let popup = centered(area, area.width.min(86), area.height.clamp(8, 18));
    let block = Block::default()
        .title(" PRINCIPAL PERMISSION FORM ")
        .borders(Borders::ALL)
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(format!("Section: {:?}", form.draft.section)),
            Line::raw(format!(
                "Operation: {}",
                if form.draft.grant { "GRANT" } else { "REVOKE" }
            )),
            Line::raw(format!("Target: {:?}", form.draft.target)),
            Line::raw(format!("Privilege: {}", form.draft.privilege)),
            Line::raw(format!(
                "Role: {}",
                form.draft.role.as_deref().unwrap_or("—")
            )),
            Line::raw(format!(
                "Option: {}",
                if form.draft.grant_option || form.draft.admin_option {
                    "ON"
                } else {
                    "OFF"
                }
            )),
            Line::styled(
                format!("Focus: {:?}", form.selected_field),
                Style::new().fg(theme.action).add_modifier(Modifier::BOLD),
            ),
        ]),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("Tab next field  Space toggle  Enter review SQL  Esc cancel"),
        chunks[1],
    );
    state.hit_regions.push(HitRegion {
        area: chunks[0],
        target: HitTarget::PrincipalMutationForm,
    });
}
