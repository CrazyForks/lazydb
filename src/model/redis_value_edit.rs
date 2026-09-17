use crate::{
    db::redis::{
        mutation::RedisValueDraft,
        read::RedisType,
        types::{RedisKeyId, RedisTarget},
    },
    identity::ConnectionIdentity,
};
use uuid::Uuid;

/// The stable identity of a value editor.  It deliberately contains the
/// Redis target and connection generation so an asynchronous save cannot be
/// applied to a different database or a newer editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisValueEditOwner {
    pub tab_id: Uuid,
    pub editor_id: Uuid,
    pub connection: ConnectionIdentity,
    pub key: RedisKeyId,
    pub preview_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisEditCapability {
    Text,
    Table,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisEditSavePhase {
    Idle,
    Confirming,
    Validating,
    Saving,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisEditSaveDecision {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisFormatSaveDecision {
    SaveValid,
    SaveInvalid,
    ReturnToEdit,
}

pub fn save_decision_for_validation(
    validation: &crate::value_preview::edit::EditValidation,
    decision: RedisFormatSaveDecision,
) -> Result<(), String> {
    match (validation, decision) {
        (crate::value_preview::edit::EditValidation::Valid, RedisFormatSaveDecision::SaveValid)
        | (
            crate::value_preview::edit::EditValidation::Warning(_),
            RedisFormatSaveDecision::SaveInvalid,
        ) => Ok(()),
        (_, RedisFormatSaveDecision::ReturnToEdit) => Err("return to editing".into()),
        (
            crate::value_preview::edit::EditValidation::Valid,
            RedisFormatSaveDecision::SaveInvalid,
        ) => Err("invalid-save decision is only valid after a format warning".into()),
        (
            crate::value_preview::edit::EditValidation::Warning(error),
            RedisFormatSaveDecision::SaveValid,
        ) => Err(format!("format validation failed: {error}")),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisValueEditDraft {
    pub owner: RedisValueEditOwner,
    pub target: RedisTarget,
    pub value_type: RedisType,
    pub capability: RedisEditCapability,
    pub baseline: Option<RedisValueDraft>,
    pub saved_text: String,
    pub current_text: String,
    pub current_revision: u64,
    pub save_phase: RedisEditSavePhase,
    pub request_id: Option<u64>,
    pub submitted_revision: Option<u64>,
    pub error: Option<String>,
}

impl RedisValueEditDraft {
    pub fn new(
        owner: RedisValueEditOwner,
        value_type: RedisType,
        capability: RedisEditCapability,
        text: impl Into<String>,
    ) -> Self {
        let text = text.into();
        Self {
            target: owner.key.target.clone(),
            owner,
            value_type,
            capability,
            baseline: None,
            saved_text: text.clone(),
            current_text: text,
            current_revision: 0,
            save_phase: RedisEditSavePhase::Idle,
            request_id: None,
            submitted_revision: None,
            error: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_text != self.current_text
    }

    pub fn update_text(&mut self, text: impl Into<String>) {
        self.current_text = text.into();
        self.current_revision = self.current_revision.saturating_add(1);
        self.error = None;
        if self.save_phase == RedisEditSavePhase::Failed {
            self.save_phase = RedisEditSavePhase::Idle;
        }
    }

    pub fn begin_save(&mut self, request_id: u64) -> Option<u64> {
        if !self.is_dirty() || self.save_phase == RedisEditSavePhase::Saving {
            return None;
        }
        self.request_id = Some(request_id);
        self.submitted_revision = Some(self.current_revision);
        self.save_phase = RedisEditSavePhase::Saving;
        self.error = None;
        Some(self.current_revision)
    }

    pub fn begin_confirmation(&mut self) -> bool {
        if self.is_dirty() && self.save_phase == RedisEditSavePhase::Idle {
            self.save_phase = RedisEditSavePhase::Confirming;
            true
        } else {
            false
        }
    }

    pub fn cancel_confirmation(&mut self) {
        if self.save_phase == RedisEditSavePhase::Confirming {
            self.save_phase = RedisEditSavePhase::Idle;
        }
    }

    /// Applies the decision made by the value-save confirmation UI.  Saving
    /// deliberately only changes the phase; the caller must still validate
    /// and submit the captured text through the Redis mutation pipeline.
    pub fn decide_save(&mut self, decision: RedisEditSaveDecision) -> bool {
        if self.save_phase != RedisEditSavePhase::Confirming {
            return false;
        }
        match decision {
            RedisEditSaveDecision::Save => {
                self.save_phase = RedisEditSavePhase::Validating;
                true
            }
            RedisEditSaveDecision::Discard => {
                self.discard();
                true
            }
            RedisEditSaveDecision::Cancel => {
                self.cancel_confirmation();
                true
            }
        }
    }

    pub fn discard(&mut self) {
        self.current_text = self.saved_text.clone();
        self.current_revision = self.current_revision.saturating_add(1);
        self.save_phase = RedisEditSavePhase::Idle;
        self.error = None;
    }

    pub fn save_succeeded(&mut self, request_id: u64, revision: u64) -> bool {
        if self.request_id != Some(request_id)
            || self.submitted_revision != Some(revision)
            || self.current_revision != revision
        {
            return false;
        }
        self.saved_text = self.current_text.clone();
        self.save_phase = RedisEditSavePhase::Idle;
        self.request_id = None;
        self.submitted_revision = None;
        self.error = None;
        true
    }

    pub fn save_failed(&mut self, request_id: u64, message: impl Into<String>) -> bool {
        if self.request_id != Some(request_id) {
            return false;
        }
        self.save_phase = RedisEditSavePhase::Failed;
        self.request_id = None;
        self.submitted_revision = None;
        self.error = Some(message.into());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> RedisValueEditDraft {
        let profile_id = Uuid::from_u128(7);
        RedisValueEditDraft::new(
            RedisValueEditOwner {
                tab_id: Uuid::from_u128(1),
                editor_id: Uuid::from_u128(2),
                connection: ConnectionIdentity {
                    profile_id,
                    generation: 3,
                },
                key: RedisKeyId {
                    target: RedisTarget {
                        profile_id,
                        database: 0,
                    },
                    key: b"key".to_vec(),
                },
                preview_generation: 1,
            },
            RedisType::String,
            RedisEditCapability::Text,
            "old",
        )
    }

    #[test]
    fn undoing_to_saved_text_is_clean() {
        let mut value = draft();
        value.update_text("new");
        assert!(value.is_dirty());
        value.update_text("old");
        assert!(!value.is_dirty());
    }

    #[test]
    fn stale_save_result_cannot_clear_newer_edit() {
        let mut value = draft();
        value.update_text("new");
        value.begin_save(4).unwrap();
        value.update_text("newer");
        assert!(!value.save_succeeded(4, 1));
        assert!(value.is_dirty());
    }

    #[test]
    fn discard_restores_the_saved_value() {
        let mut value = draft();
        value.update_text("new");
        value.discard();
        assert_eq!(value.current_text, "old");
        assert!(!value.is_dirty());
    }

    #[test]
    fn save_confirmation_decisions_have_distinct_effects() {
        let mut value = draft();
        value.update_text("new");
        assert!(value.begin_confirmation());
        assert!(value.decide_save(RedisEditSaveDecision::Cancel));
        assert_eq!(value.save_phase, RedisEditSavePhase::Idle);
        assert!(value.is_dirty());

        assert!(value.begin_confirmation());
        assert!(value.decide_save(RedisEditSaveDecision::Discard));
        assert_eq!(value.save_phase, RedisEditSavePhase::Idle);
        assert!(!value.is_dirty());

        value.update_text("newer");
        assert!(value.begin_confirmation());
        assert!(value.decide_save(RedisEditSaveDecision::Save));
        assert_eq!(value.save_phase, RedisEditSavePhase::Validating);
        assert!(value.is_dirty());
    }

    #[test]
    fn invalid_json_requires_explicit_continue_or_return() {
        let warning = crate::value_preview::edit::EditValidation::Warning("bad json".into());
        assert!(
            save_decision_for_validation(&warning, RedisFormatSaveDecision::SaveInvalid).is_ok()
        );
        assert!(
            save_decision_for_validation(&warning, RedisFormatSaveDecision::SaveValid).is_err()
        );
        assert!(
            save_decision_for_validation(&warning, RedisFormatSaveDecision::ReturnToEdit).is_err()
        );
    }
}
