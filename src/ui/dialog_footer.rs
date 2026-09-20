#![allow(dead_code)] // Removed incrementally as windows adopt the shared footer.

use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FooterDensity {
    Standard,
    Compact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FooterLayout {
    pub status_and_actions: Rect,
    pub help: Rect,
    pub content_separator: Option<Rect>,
    pub help_separator: Option<Rect>,
    pub density: FooterDensity,
}

impl FooterLayout {
    pub(crate) fn height(self) -> u16 {
        self.status_and_actions
            .height
            .saturating_add(self.help.height)
            .saturating_add(u16::from(self.content_separator.is_some()))
            .saturating_add(u16::from(self.help_separator.is_some()))
    }

    pub(crate) fn status_width(self, action_width: u16, gap: u16) -> u16 {
        self.status_and_actions
            .width
            .saturating_sub(action_width.saturating_add(gap))
    }

    pub(crate) fn status_area(self, action_width: u16, gap: u16) -> Rect {
        Rect::new(
            self.status_and_actions.x,
            self.status_and_actions.y,
            self.status_width(action_width, gap),
            self.status_and_actions.height,
        )
    }

    pub(crate) fn action_area(self, action_width: u16, _gap: u16) -> Rect {
        let width = action_width.min(self.status_and_actions.width);
        Rect::new(
            self.status_and_actions.right().saturating_sub(width),
            self.status_and_actions.y,
            width,
            self.status_and_actions.height,
        )
    }
}

pub(crate) fn measure(
    area: Rect,
    _action_width: u16,
    help_height: u16,
    density: FooterDensity,
) -> FooterLayout {
    if area.is_empty() {
        return FooterLayout {
            status_and_actions: Rect::default(),
            help: Rect::default(),
            content_separator: None,
            help_separator: None,
            density,
        };
    }

    let standard_extra = 2_u16;
    let requested_help = help_height.max(1);
    let requested_height = requested_help
        .saturating_add(1)
        .saturating_add(standard_extra);
    let use_standard = density == FooterDensity::Standard && area.height >= requested_height;
    let actual_density = if use_standard {
        FooterDensity::Standard
    } else {
        FooterDensity::Compact
    };
    let separators = u16::from(actual_density == FooterDensity::Standard) * 2;
    let help_height = requested_help
        .min(area.height.saturating_sub(1 + separators))
        .max(1.min(area.height));
    let total_height = help_height
        .saturating_add(1)
        .saturating_add(separators)
        .min(area.height);
    let top = area.bottom().saturating_sub(total_height);
    let action_y = top.saturating_add(u16::from(actual_density == FooterDensity::Standard));
    let help_y = action_y.saturating_add(u16::from(actual_density == FooterDensity::Standard) + 1);

    FooterLayout {
        status_and_actions: Rect::new(area.x, action_y, area.width, 1),
        help: Rect::new(
            area.x,
            help_y,
            area.width,
            help_height.min(area.bottom().saturating_sub(help_y)),
        ),
        content_separator: (actual_density == FooterDensity::Standard)
            .then(|| Rect::new(area.x, top, area.width, 1)),
        help_separator: (actual_density == FooterDensity::Standard)
            .then(|| Rect::new(area.x, help_y.saturating_sub(1), area.width, 1)),
        density: actual_density,
    }
}

pub(crate) fn action_width(labels: &[&str], gap: u16) -> u16 {
    labels
        .iter()
        .map(|label| label.width() as u16)
        .sum::<u16>()
        .saturating_add(gap.saturating_mul(labels.len().saturating_sub(1) as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_layout_reserves_separators_and_help() {
        let layout = measure(Rect::new(0, 0, 80, 10), 28, 1, FooterDensity::Standard);

        assert_eq!(layout.density, FooterDensity::Standard);
        assert_eq!(layout.height(), 4);
        assert_eq!(layout.status_and_actions.y, 7);
        assert_eq!(layout.help.y, 9);
        assert!(layout.content_separator.is_some());
        assert!(layout.help_separator.is_some());
    }

    #[test]
    fn compact_layout_fits_when_standard_footer_does_not() {
        let layout = measure(Rect::new(0, 0, 40, 2), 20, 1, FooterDensity::Standard);

        assert_eq!(layout.density, FooterDensity::Compact);
        assert_eq!(layout.height(), 2);
        assert!(layout.content_separator.is_none());
        assert!(layout.help_separator.is_none());
    }

    #[test]
    fn action_and_status_areas_keep_a_gap_and_use_display_width() {
        let layout = measure(Rect::new(2, 4, 40, 4), 18, 1, FooterDensity::Standard);

        assert_eq!(layout.status_width(18, 2), 20);
        assert_eq!(
            layout.status_area(18, 2).right() + 2,
            layout.action_area(18, 2).x
        );
        assert_eq!(action_width(&["[ 保存 ]", "[ Cancel ]"], 2), 20);
    }

    #[test]
    fn empty_area_is_safe() {
        let layout = measure(Rect::new(0, 0, 0, 0), 10, 1, FooterDensity::Standard);

        assert_eq!(layout.height(), 0);
        assert!(layout.help.is_empty());
    }
}
