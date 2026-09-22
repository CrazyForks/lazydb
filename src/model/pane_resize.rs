use super::{pane_navigation::PaneDirection, workspace::PaneSplit};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmartResizeDecision {
    Internal { split: PaneSplit, size: u16 },
    Boundary(PaneDirection),
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmartResizePane {
    Explorer,
    Editor,
    Results,
    Relation,
    RedisKeys,
    RedisPreview,
}

pub fn decide(
    pane: SmartResizePane,
    direction: PaneDirection,
    explorer_width: Option<(u16, u16, u16)>,
    editor_height: Option<(u16, u16, u16)>,
    redis_keys_width: Option<(u16, u16, u16)>,
) -> SmartResizeDecision {
    let internal = match (pane, direction) {
        (SmartResizePane::Explorer, PaneDirection::Right) => {
            explorer_width.and_then(|(current, _, maximum)| {
                (current < maximum).then_some((
                    PaneSplit::ExplorerWidth,
                    current.saturating_add(3).min(maximum),
                ))
            })
        }
        (SmartResizePane::Editor, PaneDirection::Left) => {
            explorer_width.and_then(|(current, minimum, _)| {
                (current > minimum).then_some((
                    PaneSplit::ExplorerWidth,
                    current.saturating_sub(3).max(minimum),
                ))
            })
        }
        (SmartResizePane::Results, PaneDirection::Left)
        | (SmartResizePane::Relation, PaneDirection::Left)
        | (SmartResizePane::RedisKeys, PaneDirection::Left) => {
            explorer_width.and_then(|(current, minimum, _)| {
                (current > minimum).then_some((
                    PaneSplit::ExplorerWidth,
                    current.saturating_sub(3).max(minimum),
                ))
            })
        }
        (SmartResizePane::Editor, PaneDirection::Down) => {
            editor_height.and_then(|(current, _, maximum)| {
                (current < maximum).then_some((
                    PaneSplit::EditorHeight,
                    current.saturating_add(3).min(maximum),
                ))
            })
        }
        (SmartResizePane::Results, PaneDirection::Up) => {
            editor_height.and_then(|(current, minimum, _)| {
                (current > minimum).then_some((
                    PaneSplit::EditorHeight,
                    current.saturating_sub(3).max(minimum),
                ))
            })
        }
        (SmartResizePane::RedisKeys, PaneDirection::Right) => {
            redis_keys_width.and_then(|(current, _, maximum)| {
                (current < maximum).then_some((
                    PaneSplit::RedisKeysWidth,
                    current.saturating_add(3).min(maximum),
                ))
            })
        }
        (SmartResizePane::RedisPreview, PaneDirection::Left) => {
            redis_keys_width.and_then(|(current, minimum, _)| {
                (current > minimum).then_some((
                    PaneSplit::RedisKeysWidth,
                    current.saturating_sub(3).max(minimum),
                ))
            })
        }
        _ => None,
    };
    if let Some((split, size)) = internal {
        return SmartResizeDecision::Internal { split, size };
    }
    SmartResizeDecision::Boundary(direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Sizes = (
        Option<(u16, u16, u16)>,
        Option<(u16, u16, u16)>,
        Option<(u16, u16, u16)>,
    );

    fn sizes() -> Sizes {
        (Some((50, 34, 59)), Some((20, 5, 30)), Some((30, 16, 50)))
    }

    #[test]
    fn explorer_right_prefers_internal_boundary() {
        let (explorer, editor, redis) = sizes();
        assert_eq!(
            decide(
                SmartResizePane::Explorer,
                PaneDirection::Right,
                explorer,
                editor,
                redis
            ),
            SmartResizeDecision::Internal {
                split: PaneSplit::ExplorerWidth,
                size: 53
            }
        );
    }

    #[test]
    fn right_edge_editor_falls_back_to_kitty() {
        let (explorer, editor, redis) = sizes();
        assert_eq!(
            decide(
                SmartResizePane::Editor,
                PaneDirection::Right,
                explorer,
                editor,
                redis
            ),
            SmartResizeDecision::Boundary(PaneDirection::Right)
        );
    }

    #[test]
    fn unavailable_boundary_falls_back() {
        let (_, editor, redis) = sizes();
        assert_eq!(
            decide(
                SmartResizePane::Explorer,
                PaneDirection::Right,
                None,
                editor,
                redis
            ),
            SmartResizeDecision::Boundary(PaneDirection::Right)
        );
    }

    #[test]
    fn vertical_boundaries_resize_the_adjacent_internal_split() {
        let (explorer, editor, redis) = sizes();
        assert_eq!(
            decide(
                SmartResizePane::Editor,
                PaneDirection::Down,
                explorer,
                editor,
                redis
            ),
            SmartResizeDecision::Internal {
                split: PaneSplit::EditorHeight,
                size: 23
            }
        );
        assert_eq!(
            decide(
                SmartResizePane::Results,
                PaneDirection::Up,
                explorer,
                editor,
                redis
            ),
            SmartResizeDecision::Internal {
                split: PaneSplit::EditorHeight,
                size: 17
            }
        );
    }

    #[test]
    fn dimensions_at_limits_fall_back_instead_of_emitting_a_noop() {
        assert_eq!(
            decide(
                SmartResizePane::Explorer,
                PaneDirection::Right,
                Some((59, 34, 59)),
                None,
                None
            ),
            SmartResizeDecision::Boundary(PaneDirection::Right)
        );
        assert_eq!(
            decide(
                SmartResizePane::Editor,
                PaneDirection::Down,
                None,
                Some((5, 5, 5)),
                None
            ),
            SmartResizeDecision::Boundary(PaneDirection::Down)
        );
    }
}
