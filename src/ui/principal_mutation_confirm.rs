use crate::{
    db::principal::PrincipalMutationPlan, model::workspace::PrincipalMutationConfirmFocus,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::{HitRegion, HitTarget, UiState, centered, theme::Theme};

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    plan: &PrincipalMutationPlan,
    focus: PrincipalMutationConfirmFocus,
    state: &mut UiState,
    theme: Theme,
) {
    let popup = centered(area, area.width.min(92), area.height.clamp(8, 18));
    let block = Block::default()
        .title(" REVIEW PRINCIPAL SQL ")
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "{}  {}",
                    plan.principal.name,
                    plan.database.as_deref().unwrap_or("current database")
                ),
                theme.title(true),
            )),
            Line::styled(
                "Privilege changes are not applied until Apply is confirmed.",
                Style::new().fg(theme.warning),
            ),
        ]),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(plan.sql.as_str())
            .wrap(Wrap { trim: false })
            .block(Block::default().title(" SQL ").borders(Borders::ALL)),
        chunks[1],
    );
    let labels = ["Cancel", "Apply"];
    let width = chunks[2].width / 2;
    for (index, label) in labels.into_iter().enumerate() {
        let rect = Rect::new(chunks[2].x + width * index as u16, chunks[2].y, width, 1);
        let selected = matches!(
            (index, focus),
            (0, PrincipalMutationConfirmFocus::Cancel) | (1, PrincipalMutationConfirmFocus::Apply)
        );
        frame.render_widget(
            Paragraph::new(format!("[ {label} ]")).style(if selected {
                theme.title(true).add_modifier(Modifier::REVERSED)
            } else {
                Style::new().fg(theme.muted)
            }),
            rect,
        );
        state.hit_regions.push(HitRegion {
            area: rect,
            target: if index == 0 {
                HitTarget::PrincipalMutationCancel
            } else {
                HitTarget::PrincipalMutationApply
            },
        });
    }
}
