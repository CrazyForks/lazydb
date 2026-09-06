//! Shared focus behavior for confirmation controls.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmationFocus {
    Input,
    Action(usize),
}

impl ConfirmationFocus {
    pub fn next(self, action_count: usize, input_enabled: bool) -> Self {
        self.move_by(1, action_count, input_enabled)
    }

    pub fn previous(self, action_count: usize, input_enabled: bool) -> Self {
        self.move_by(-1, action_count, input_enabled)
    }

    pub fn move_by(self, delta: isize, action_count: usize, input_enabled: bool) -> Self {
        let mut controls = Vec::with_capacity(action_count + usize::from(input_enabled));
        if input_enabled {
            controls.push(Self::Input);
        }
        controls.extend((0..action_count).map(Self::Action));
        if controls.is_empty() {
            return Self::Action(0);
        }
        let current = controls
            .iter()
            .position(|focus| *focus == self)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(controls.len() as isize) as usize;
        controls[next]
    }
}

#[cfg(test)]
mod tests {
    use super::ConfirmationFocus;

    #[test]
    fn cycles_input_and_actions_in_visual_order() {
        assert_eq!(
            ConfirmationFocus::Input.next(2, true),
            ConfirmationFocus::Action(0)
        );
        assert_eq!(
            ConfirmationFocus::Action(1).next(2, true),
            ConfirmationFocus::Input
        );
        assert_eq!(
            ConfirmationFocus::Input.previous(2, true),
            ConfirmationFocus::Action(1)
        );
    }

    #[test]
    fn action_only_focus_wraps() {
        assert_eq!(
            ConfirmationFocus::Action(0).previous(2, false),
            ConfirmationFocus::Action(1)
        );
        assert_eq!(
            ConfirmationFocus::Action(1).next(2, false),
            ConfirmationFocus::Action(0)
        );
    }
}
