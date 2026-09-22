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
    fn resize(
        split: PaneSplit,
        current: u16,
        minimum: u16,
        maximum: u16,
        increase: bool,
    ) -> Option<(PaneSplit, u16)> {
        let next = if increase {
            current.saturating_add(3).min(maximum)
        } else {
            current.saturating_sub(3).max(minimum)
        };
        (next != current).then_some((split, next))
    }

    let internal = match (pane, direction) {
        (SmartResizePane::Explorer, PaneDirection::Left)
        | (SmartResizePane::Editor, PaneDirection::Left)
        | (SmartResizePane::Results, PaneDirection::Left)
        | (SmartResizePane::Relation, PaneDirection::Left) => {
            explorer_width.and_then(|(current, minimum, maximum)| {
                resize(PaneSplit::ExplorerWidth, current, minimum, maximum, false)
            })
        }
        (SmartResizePane::RedisKeys, PaneDirection::Left) => redis_keys_width.map_or_else(
            || {
                explorer_width.and_then(|(current, minimum, maximum)| {
                    resize(PaneSplit::ExplorerWidth, current, minimum, maximum, false)
                })
            },
            |(current, minimum, maximum)| {
                resize(PaneSplit::RedisKeysWidth, current, minimum, maximum, false)
            },
        ),
        (SmartResizePane::Explorer, PaneDirection::Right)
        | (SmartResizePane::Editor, PaneDirection::Right)
        | (SmartResizePane::Results, PaneDirection::Right)
        | (SmartResizePane::Relation, PaneDirection::Right) => {
            explorer_width.and_then(|(current, minimum, maximum)| {
                resize(PaneSplit::ExplorerWidth, current, minimum, maximum, true)
            })
        }
        (SmartResizePane::RedisKeys, PaneDirection::Right) => redis_keys_width.map_or_else(
            || {
                explorer_width.and_then(|(current, minimum, maximum)| {
                    resize(PaneSplit::ExplorerWidth, current, minimum, maximum, true)
                })
            },
            |(current, minimum, maximum)| {
                resize(PaneSplit::RedisKeysWidth, current, minimum, maximum, true)
            },
        ),
        (SmartResizePane::RedisPreview, PaneDirection::Left) => {
            redis_keys_width.and_then(|(current, minimum, maximum)| {
                resize(PaneSplit::RedisKeysWidth, current, minimum, maximum, false)
            })
        }
        (SmartResizePane::RedisPreview, PaneDirection::Right) => {
            redis_keys_width.and_then(|(current, minimum, maximum)| {
                resize(PaneSplit::RedisKeysWidth, current, minimum, maximum, true)
            })
        }
        (SmartResizePane::Editor, PaneDirection::Up)
        | (SmartResizePane::Results, PaneDirection::Up) => {
            editor_height.and_then(|(current, minimum, maximum)| {
                resize(PaneSplit::EditorHeight, current, minimum, maximum, false)
            })
        }
        (SmartResizePane::Editor, PaneDirection::Down)
        | (SmartResizePane::Results, PaneDirection::Down) => {
            editor_height.and_then(|(current, minimum, maximum)| {
                resize(PaneSplit::EditorHeight, current, minimum, maximum, true)
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
    fn editor_horizontal_resize_moves_explorer_boundary() {
        let (explorer, editor, redis) = sizes();
        assert_eq!(
            decide(
                SmartResizePane::Editor,
                PaneDirection::Right,
                explorer,
                editor,
                redis
            ),
            SmartResizeDecision::Internal {
                split: PaneSplit::ExplorerWidth,
                size: 53,
            }
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
    fn explorer_and_editor_resize_in_both_directions() {
        let (explorer, editor, redis) = sizes();
        assert_eq!(
            decide(
                SmartResizePane::Explorer,
                PaneDirection::Left,
                explorer,
                editor,
                redis
            ),
            SmartResizeDecision::Internal {
                split: PaneSplit::ExplorerWidth,
                size: 47,
            }
        );
        assert_eq!(
            decide(
                SmartResizePane::Editor,
                PaneDirection::Up,
                explorer,
                editor,
                redis
            ),
            SmartResizeDecision::Internal {
                split: PaneSplit::EditorHeight,
                size: 17,
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
