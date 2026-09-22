use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneDirection {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum PaneTarget {
    Explorer,
    Editor,
    Results,
    Relation,
    RedisKeys,
    RedisPreview,
}

impl PaneTarget {
    fn order(self) -> u8 {
        match self {
            Self::Explorer => 0,
            Self::Editor => 1,
            Self::Results => 2,
            Self::Relation => 3,
            Self::RedisKeys => 4,
            Self::RedisPreview => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneNavigation {
    Internal(PaneTarget),
    Boundary(PaneDirection),
    Blocked,
}

fn overlaps(a_start: u16, a_end: u16, b_start: u16, b_end: u16) -> bool {
    a_start < b_end && b_start < a_end
}

pub fn navigate(
    source: PaneTarget,
    direction: PaneDirection,
    panes: &[(PaneTarget, Rect)],
) -> PaneNavigation {
    let Some((_, current)) = panes.iter().find(|(target, _)| *target == source) else {
        return PaneNavigation::Blocked;
    };

    let mut candidates = panes.iter().filter_map(|(target, rect)| {
        if *target == source || rect.width == 0 || rect.height == 0 {
            return None;
        }
        let (in_direction, distance, overlap_start) = match direction {
            PaneDirection::Left if rect.right() <= current.x => (
                true,
                current.x.saturating_sub(rect.right()),
                rect.y.max(current.y),
            ),
            PaneDirection::Right if rect.x >= current.right() => (
                true,
                rect.x.saturating_sub(current.right()),
                rect.y.max(current.y),
            ),
            PaneDirection::Up if rect.bottom() <= current.y => (
                true,
                current.y.saturating_sub(rect.bottom()),
                rect.x.max(current.x),
            ),
            PaneDirection::Down if rect.y >= current.bottom() => (
                true,
                rect.y.saturating_sub(current.bottom()),
                rect.x.max(current.x),
            ),
            _ => (false, 0, 0),
        };
        let aligned = match direction {
            PaneDirection::Left | PaneDirection::Right => {
                overlaps(current.y, current.bottom(), rect.y, rect.bottom())
            }
            PaneDirection::Up | PaneDirection::Down => {
                overlaps(current.x, current.right(), rect.x, rect.right())
            }
        };
        (in_direction && aligned).then_some((*target, distance, overlap_start))
    });
    candidates
        .next()
        .map(|first| {
            candidates
                .fold(first, |best, candidate| {
                    if (candidate.1, candidate.2)
                        .cmp(&(best.1, best.2))
                        .then_with(|| candidate.0.order().cmp(&best.0.order()))
                        .is_lt()
                    {
                        candidate
                    } else {
                        best
                    }
                })
                .0
        })
        .map(PaneNavigation::Internal)
        .unwrap_or(PaneNavigation::Boundary(direction))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn smart_pane_right_prefers_the_nearest_aligned_pane() {
        let panes = [
            (PaneTarget::Explorer, Rect::new(0, 0, 20, 30)),
            (PaneTarget::Editor, Rect::new(20, 0, 60, 15)),
            (PaneTarget::Results, Rect::new(20, 15, 60, 15)),
        ];
        assert_eq!(
            navigate(PaneTarget::Explorer, PaneDirection::Right, &panes),
            PaneNavigation::Internal(PaneTarget::Editor)
        );
    }

    #[test]
    fn smart_pane_right_reports_boundary() {
        let panes = [(PaneTarget::Editor, Rect::new(0, 0, 80, 20))];
        assert_eq!(
            navigate(PaneTarget::Editor, PaneDirection::Right, &panes),
            PaneNavigation::Boundary(PaneDirection::Right)
        );
    }
}
