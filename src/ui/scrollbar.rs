use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScrollbarGeometry {
    pub rail: Rect,
    pub thumb_start: u16,
    pub thumb_length: u16,
    pub max_offset: usize,
}

impl ScrollbarGeometry {
    pub(crate) fn thumb_area(self) -> Rect {
        if self.rail.width >= self.rail.height {
            Rect::new(
                self.rail.x.saturating_add(self.thumb_start),
                self.rail.y,
                self.thumb_length,
                self.rail.height,
            )
        } else {
            Rect::new(
                self.rail.x,
                self.rail.y.saturating_add(self.thumb_start),
                self.rail.width,
                self.thumb_length,
            )
        }
    }

    pub(crate) fn offset_at(self, pointer: u16, pointer_offset: u16) -> usize {
        let (start, length) = if self.rail.width >= self.rail.height {
            (self.rail.x, self.rail.width)
        } else {
            (self.rail.y, self.rail.height)
        };
        let travel = length.saturating_sub(self.thumb_length);
        if travel == 0 || self.max_offset == 0 {
            return 0;
        }
        let position = pointer
            .saturating_sub(start)
            .saturating_sub(pointer_offset)
            .min(travel);
        ((u128::from(position) * self.max_offset as u128 + u128::from(travel) / 2)
            / u128::from(travel))
        .min(self.max_offset as u128) as usize
    }
}

pub(crate) fn geometry(
    rail: Rect,
    visible: usize,
    content: usize,
    offset: usize,
) -> Option<ScrollbarGeometry> {
    let (length, cross) = if rail.width >= rail.height {
        (rail.width, rail.height)
    } else {
        (rail.height, rail.width)
    };
    if length < 3 || cross == 0 || visible == 0 || content <= visible {
        return None;
    }
    let usable = length.saturating_sub(2);
    if usable == 0 {
        return None;
    }
    let max_offset = content.saturating_sub(visible);
    let thumb_length = (((u128::from(usable) * visible as u128) / content as u128)
        .clamp(1, u128::from(usable))) as u16;
    let travel = usable.saturating_sub(thumb_length);
    let thumb_start = if travel == 0 || max_offset == 0 {
        0
    } else {
        ((u128::from(travel) * offset.min(max_offset) as u128) / max_offset as u128) as u16
    };
    let rail = if rail.width >= rail.height {
        Rect::new(rail.x.saturating_add(1), rail.y, usable, rail.height)
    } else {
        Rect::new(rail.x, rail.y.saturating_add(1), rail.width, usable)
    };
    Some(ScrollbarGeometry {
        rail,
        thumb_start,
        thumb_length,
        max_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::{ScrollbarGeometry, geometry};
    use ratatui::layout::Rect;

    #[test]
    fn geometry_uses_the_full_track_and_clamps_the_thumb() {
        let horizontal = geometry(Rect::new(0, 0, 12, 1), 2, 10, 100).unwrap();
        assert_eq!(horizontal.rail, Rect::new(1, 0, 10, 1));
        assert_eq!(horizontal.thumb_length, 2);
        assert_eq!(horizontal.thumb_start, 8);

        let vertical = geometry(Rect::new(4, 2, 1, 12), 2, 10, 100).unwrap();
        assert_eq!(vertical.rail, Rect::new(4, 3, 1, 10));
        assert_eq!(vertical.thumb_length, 2);
        assert_eq!(vertical.thumb_start, 8);
    }

    #[test]
    fn geometry_hides_when_there_is_no_scrollable_content() {
        assert!(geometry(Rect::new(0, 0, 2, 1), 1, 2, 0).is_none());
        assert!(geometry(Rect::new(0, 0, 10, 1), 10, 10, 0).is_none());
        assert!(geometry(Rect::new(0, 0, 10, 1), 0, 10, 0).is_none());
    }

    #[test]
    fn offset_at_preserves_the_pointer_offset_inside_the_thumb() {
        let geometry = geometry(Rect::new(0, 0, 12, 1), 2, 10, 4).unwrap();
        assert_eq!(geometry.offset_at(geometry.thumb_area().x + 1, 1), 4);
        assert_eq!(geometry.offset_at(100, 0), geometry.max_offset);
    }

    #[test]
    fn geometry_handles_large_ranges_without_overflow() {
        let geometry = geometry(
            Rect::new(0, 0, u16::MAX, 1),
            usize::MAX / 4,
            usize::MAX / 2,
            usize::MAX,
        )
        .unwrap();
        assert!(geometry.thumb_length > 0);
        assert_eq!(geometry.offset_at(u16::MAX, 0), geometry.max_offset);
    }

    #[test]
    fn thumb_area_matches_the_axis_of_the_rail() {
        let horizontal = ScrollbarGeometry {
            rail: Rect::new(1, 2, 10, 1),
            thumb_start: 3,
            thumb_length: 2,
            max_offset: 1,
        };
        assert_eq!(horizontal.thumb_area(), Rect::new(4, 2, 2, 1));
        let vertical = ScrollbarGeometry {
            rail: Rect::new(1, 2, 1, 10),
            thumb_start: 3,
            thumb_length: 2,
            max_offset: 1,
        };
        assert_eq!(vertical.thumb_area(), Rect::new(1, 5, 1, 2));
    }
}
