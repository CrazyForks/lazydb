use crate::{
    db::redis::types::{RedisKeyId, RedisTarget},
    db::redis::{mutation::RedisMutationOperation, read::RedisType},
    identity::ConnectionIdentity,
    model::text_input::{TextInput, TextInputEdit},
    value_preview::table::RedisTableRow,
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisTableEditorMode {
    Edit,
    Add,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisTableEditorState {
    pub tab_id: Uuid,
    pub connection: ConnectionIdentity,
    pub key: RedisKeyId,
    pub request_id: u64,
    pub busy: bool,
    pub plan: Option<crate::db::redis::mutation::RedisMutationPlan>,
    pub operation_override: Option<RedisMutationOperation>,
    pub value_type: RedisType,
    pub mode: RedisTableEditorMode,
    pub row: Option<RedisTableRow>,
    pub fields: Vec<TextInput>,
    pub focused: usize,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisTableDeleteFocus {
    Cancel,
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisTableDeleteConfirmation {
    pub tab_id: Uuid,
    pub key: RedisKeyId,
    pub value_type: RedisType,
    pub row: RedisTableRow,
    pub focus: RedisTableDeleteFocus,
}

impl RedisTableDeleteConfirmation {
    pub fn title(&self) -> String {
        format!(
            "Redis {:?}: {}",
            self.value_type,
            crate::ui::redis_value::display_bytes_lossless(&self.key.key)
        )
    }
}

impl RedisTableEditorState {
    pub fn edit(
        tab_id: Uuid,
        connection: ConnectionIdentity,
        key: RedisKeyId,
        value_type: RedisType,
        row: RedisTableRow,
    ) -> Self {
        let fields = row
            .identity
            .iter()
            .map(|value| TextInput::from(crate::ui::redis_value::display_bytes_lossless(value)))
            .collect();
        Self {
            tab_id,
            connection,
            key,
            request_id: 0,
            busy: false,
            plan: None,
            operation_override: None,
            value_type,
            mode: RedisTableEditorMode::Edit,
            row: Some(row),
            fields,
            focused: 0,
            error: None,
        }
    }

    pub fn add(
        tab_id: Uuid,
        connection: ConnectionIdentity,
        key: RedisKeyId,
        target: RedisTarget,
        value_type: RedisType,
        columns: usize,
    ) -> Self {
        Self {
            tab_id,
            connection,
            key: RedisKeyId { target, ..key },
            request_id: 0,
            busy: false,
            plan: None,
            operation_override: None,
            value_type,
            mode: RedisTableEditorMode::Add,
            row: None,
            fields: (0..columns).map(|_| TextInput::default()).collect(),
            focused: 0,
            error: None,
        }
    }

    pub fn next_field(&mut self, delta: isize) {
        if self.fields.is_empty() {
            return;
        }
        self.focused =
            (self.focused as isize + delta).rem_euclid(self.fields.len() as isize) as usize;
    }

    pub fn edit_focused(&mut self, edit: TextInputEdit) {
        if self.busy {
            return;
        }
        if let Some(field) = self.fields.get_mut(self.focused) {
            field.apply(edit);
            self.error = None;
            self.plan = None;
        }
    }

    pub fn values(&self) -> Result<Vec<Vec<u8>>, String> {
        self.fields
            .iter()
            .map(|field| crate::model::redis_object_editor::parse_bytes(field.value()))
            .collect()
    }

    pub fn operation(&self) -> Result<RedisMutationOperation, String> {
        if let Some(operation) = &self.operation_override {
            return Ok(operation.clone());
        }
        if self.mode == RedisTableEditorMode::Edit {
            let row = self.row.as_ref().ok_or("missing row")?;
            let value = self
                .values()?
                .get(self.focused)
                .cloned()
                .ok_or("missing focused value")?;
            return row.edit_operation(self.value_type, self.focused, value);
        }
        let values = self.values()?;
        match self.value_type {
            RedisType::Hash if values.len() >= 2 => Ok(RedisMutationOperation::AddHashField {
                field: values[0].clone(),
                value: values[1].clone(),
                // The adapter's Add path must reject an existing field. The
                // absence expectation is represented by None at the command
                // layer today, so the app validates the target before
                // dispatching this operation.
            }),
            RedisType::Set => values
                .first()
                .cloned()
                .map(|member| RedisMutationOperation::AddSetMember { member })
                .ok_or_else(|| "a set member is required".into()),
            RedisType::List => values
                .get(1)
                .cloned()
                .map(|value| RedisMutationOperation::AppendListElement { value })
                .ok_or_else(|| "a list value is required".into()),
            RedisType::Stream if values.len() >= 2 => Ok(RedisMutationOperation::AppendStream {
                fields: vec![(values[0].clone(), values[1].clone())],
            }),
            RedisType::SortedSet if values.len() >= 2 => {
                let score = String::from_utf8(values[1].clone())
                    .map_err(|_| "sorted-set score is not UTF-8")?;
                score
                    .parse::<f64>()
                    .map_err(|_| "sorted-set score is invalid")?;
                Ok(RedisMutationOperation::AddSortedSetMember {
                    member: values[0].clone(),
                    score,
                })
            }
            _ => Err("this Redis type does not support adding a row here".into()),
        }
    }

    pub fn request(&self) -> Result<crate::db::redis::mutation::RedisMutationRequest, String> {
        if self.key.key.is_empty() {
            return Err("Redis key is required".into());
        }
        if self.request_id == 0 {
            return Err("Redis request ID is required".into());
        }
        Ok(crate::db::redis::mutation::RedisMutationRequest {
            connection: self.connection,
            request_id: self.request_id,
            mode: crate::db::redis::mutation::RedisMutationMode::Edit,
            key: self.key.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_form_preserves_columns_and_focuses_the_selected_field() {
        let row = RedisTableRow {
            cells: vec!["field".into(), "value".into()],
            identity: vec![b"field".to_vec(), b"value".to_vec()],
            row_key: b"field".to_vec(),
        };
        let profile_id = Uuid::from_u128(7);
        let mut editor = RedisTableEditorState::edit(
            Uuid::from_u128(1),
            ConnectionIdentity {
                profile_id,
                generation: 1,
            },
            RedisKeyId {
                target: RedisTarget {
                    profile_id,
                    database: 0,
                },
                key: b"key".to_vec(),
            },
            RedisType::Hash,
            row,
        );
        assert_eq!(editor.fields.len(), 2);
        assert_eq!(editor.focused, 0);
        editor.next_field(1);
        editor.edit_focused(TextInputEdit::Insert('!'));
        assert_eq!(editor.fields[1].value(), "value!");
        assert!(matches!(
            editor.operation(),
            Ok(RedisMutationOperation::SetHashField { .. })
        ));
    }

    #[test]
    fn add_form_keeps_the_opened_key_for_its_mutation_request() {
        let profile_id = Uuid::from_u128(7);
        let key = RedisKeyId {
            target: RedisTarget {
                profile_id,
                database: 0,
            },
            key: b"hash".to_vec(),
        };
        let editor = RedisTableEditorState::add(
            Uuid::from_u128(1),
            ConnectionIdentity {
                profile_id,
                generation: 1,
            },
            key,
            RedisTarget {
                profile_id,
                database: 0,
            },
            RedisType::Set,
            1,
        );
        assert_eq!(editor.key.key, b"hash");
    }

    #[test]
    fn add_form_builds_a_list_append_operation() {
        let profile_id = Uuid::from_u128(8);
        let mut editor = RedisTableEditorState::add(
            Uuid::from_u128(1),
            ConnectionIdentity {
                profile_id,
                generation: 1,
            },
            RedisKeyId {
                target: RedisTarget {
                    profile_id,
                    database: 0,
                },
                key: b"list".to_vec(),
            },
            RedisTarget {
                profile_id,
                database: 0,
            },
            RedisType::List,
            2,
        );
        editor.next_field(1);
        editor.edit_focused(TextInputEdit::Insert('x'));
        assert_eq!(
            editor.operation().unwrap(),
            RedisMutationOperation::AppendListElement {
                value: b"x".to_vec()
            }
        );
    }

    #[test]
    fn delete_confirmation_title_includes_type_and_lossless_key() {
        let profile_id = Uuid::from_u128(9);
        let confirmation = RedisTableDeleteConfirmation {
            tab_id: Uuid::from_u128(1),
            key: RedisKeyId {
                target: RedisTarget {
                    profile_id,
                    database: 2,
                },
                key: b"moss:common:api_permission:1868565521981796353".to_vec(),
            },
            value_type: RedisType::Hash,
            row: RedisTableRow {
                cells: vec!["field".into(), "value".into()],
                identity: vec![b"field".to_vec(), b"value".to_vec()],
                row_key: b"field".to_vec(),
            },
            focus: RedisTableDeleteFocus::Cancel,
        };
        assert_eq!(
            confirmation.title(),
            "Redis Hash: moss:common:api_permission:1868565521981796353"
        );
    }
}
