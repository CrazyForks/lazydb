use uuid::Uuid;

use crate::{
    db::redis::{
        mutation::{
            RedisKeyBaseline, RedisMutationMode, RedisMutationOperation, RedisMutationPlan,
            RedisMutationRequest, RedisTtlMutation, RedisValueDraft,
        },
        read::{RedisType, RedisValuePage, TtlState},
        types::{RedisKeyId, RedisTarget},
    },
    identity::ConnectionIdentity,
    model::text_input::TextInput,
};

type RedisPairDraft = (Vec<u8>, Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisObjectEditorFocus {
    Key,
    Type,
    Value,
    Ttl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisEditorTtlMode {
    Preserve,
    Persistent,
    Expires,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisObjectEditorState {
    pub tab_id: Uuid,
    pub connection: ConnectionIdentity,
    pub target: RedisTarget,
    pub mode: RedisMutationMode,
    pub key: TextInput,
    pub original_key: Option<Vec<u8>>,
    pub value_type: RedisType,
    pub value: TextInput,
    pub ttl: TextInput,
    pub ttl_mode: RedisEditorTtlMode,
    pub focus: RedisObjectEditorFocus,
    pub baseline: Option<RedisKeyBaseline>,
    pub request_id: u64,
    pub busy: bool,
    pub error: Option<String>,
    pub plan: Option<RedisMutationPlan>,
}

impl RedisObjectEditorState {
    pub fn create(tab_id: Uuid, connection: ConnectionIdentity, target: RedisTarget) -> Self {
        Self {
            tab_id,
            connection,
            target,
            mode: RedisMutationMode::Create,
            key: TextInput::default(),
            original_key: None,
            value_type: RedisType::String,
            value: TextInput::default(),
            ttl: TextInput::default(),
            ttl_mode: RedisEditorTtlMode::Persistent,
            focus: RedisObjectEditorFocus::Key,
            baseline: None,
            request_id: 0,
            busy: false,
            error: None,
            plan: None,
        }
    }

    pub fn edit(
        tab_id: Uuid,
        connection: ConnectionIdentity,
        key: RedisKeyId,
        value_type: RedisType,
        ttl: TtlState,
    ) -> Self {
        let ttl_mode = match ttl {
            TtlState::ExpiresIn { .. } => RedisEditorTtlMode::Expires,
            TtlState::Persistent | TtlState::Unavailable | TtlState::Missing => {
                RedisEditorTtlMode::Preserve
            }
        };
        let ttl_input = match ttl {
            TtlState::ExpiresIn { millis } => millis.to_string().into(),
            _ => TextInput::default(),
        };
        Self {
            tab_id,
            connection,
            target: key.target.clone(),
            mode: RedisMutationMode::Edit,
            key: display_bytes(&key.key).into(),
            original_key: Some(key.key.clone()),
            value_type,
            value: TextInput::default(),
            ttl: ttl_input,
            ttl_mode,
            focus: RedisObjectEditorFocus::Value,
            baseline: Some(RedisKeyBaseline { value_type }),
            request_id: 0,
            busy: false,
            error: None,
            plan: None,
        }
    }

    pub fn edit_from_page(
        tab_id: Uuid,
        connection: ConnectionIdentity,
        page: &RedisValuePage,
    ) -> Self {
        let mut editor = Self::edit(
            tab_id,
            connection,
            page.metadata.key.clone(),
            page.metadata.value_type,
            page.metadata.ttl,
        );
        editor.value.set(value_page_text(page));
        editor
    }

    pub fn focused_input_mut(&mut self) -> Option<&mut TextInput> {
        match self.focus {
            RedisObjectEditorFocus::Key if self.mode == RedisMutationMode::Create => {
                Some(&mut self.key)
            }
            RedisObjectEditorFocus::Key => None,
            RedisObjectEditorFocus::Value => Some(&mut self.value),
            RedisObjectEditorFocus::Ttl => Some(&mut self.ttl),
            RedisObjectEditorFocus::Type => None,
        }
    }

    pub fn move_focus(&mut self, delta: isize) {
        let fields = if self.mode == RedisMutationMode::Create {
            vec![
                RedisObjectEditorFocus::Key,
                RedisObjectEditorFocus::Type,
                RedisObjectEditorFocus::Value,
                RedisObjectEditorFocus::Ttl,
            ]
        } else {
            vec![
                RedisObjectEditorFocus::Type,
                RedisObjectEditorFocus::Value,
                RedisObjectEditorFocus::Ttl,
            ]
        };
        let current = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(fields.len() as isize) as usize;
        self.focus = fields[next];
    }

    pub fn cycle_type(&mut self, delta: isize) {
        if self.mode == RedisMutationMode::Edit {
            return;
        }
        let types = [
            RedisType::String,
            RedisType::Hash,
            RedisType::List,
            RedisType::Set,
            RedisType::SortedSet,
            RedisType::Stream,
        ];
        let current = types
            .iter()
            .position(|value_type| *value_type == self.value_type)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(types.len() as isize) as usize;
        self.value_type = types[next];
    }

    pub fn ttl_mutation(&self) -> Result<RedisTtlMutation, String> {
        match self.ttl_mode {
            RedisEditorTtlMode::Preserve => Ok(RedisTtlMutation::Preserve),
            RedisEditorTtlMode::Persistent => Ok(RedisTtlMutation::Persist),
            RedisEditorTtlMode::Expires => self
                .ttl
                .value()
                .trim()
                .parse::<u64>()
                .map(RedisTtlMutation::SetMillis)
                .map_err(|_| "TTL must be a non-negative number of milliseconds".into()),
        }
    }

    pub fn request(&self) -> Result<RedisMutationRequest, String> {
        let key = if self.mode == RedisMutationMode::Edit {
            self.original_key
                .clone()
                .ok_or_else(|| "edited Redis key identity is missing".to_owned())?
        } else {
            parse_bytes(self.key.value())?
        };
        if key.is_empty() {
            return Err("Redis key is required".into());
        }
        Ok(RedisMutationRequest {
            connection: self.connection,
            request_id: self.request_id,
            mode: self.mode,
            key: RedisKeyId {
                target: self.target.clone(),
                key,
            },
        })
    }

    pub fn operation(&self) -> Result<RedisMutationOperation, String> {
        Ok(RedisMutationOperation::Replace(self.value_draft()?))
    }

    pub fn value_draft(&self) -> Result<RedisValueDraft, String> {
        match self.value_type {
            RedisType::String => Ok(RedisValueDraft::String(parse_bytes(self.value.value())?)),
            RedisType::Hash => Ok(RedisValueDraft::Hash(parse_pairs(self.value.value())?)),
            RedisType::List => Ok(RedisValueDraft::List(parse_lines(self.value.value())?)),
            RedisType::Set => Ok(RedisValueDraft::Set(parse_lines(self.value.value())?)),
            RedisType::SortedSet => Ok(RedisValueDraft::SortedSet(
                self.value
                    .value()
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| {
                        let (score, member) = line
                            .split_once('\t')
                            .ok_or_else(|| "sorted-set rows require score<TAB>member".to_owned())?;
                        Ok((score.trim().to_owned(), parse_bytes(member)?))
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )),
            RedisType::Stream => Ok(RedisValueDraft::Stream(parse_pairs(self.value.value())?)),
            _ => Err("this Redis type cannot be edited by the native editor".into()),
        }
    }

    pub fn begin_plan(&mut self, request_id: u64) {
        self.request_id = request_id;
        self.busy = true;
        self.error = None;
        self.plan = None;
    }

    pub fn plan_ready(&mut self, plan: RedisMutationPlan) {
        self.busy = false;
        self.error = None;
        self.plan = Some(plan);
    }

    pub fn plan_failed(&mut self, message: impl Into<String>) {
        self.busy = false;
        self.error = Some(message.into());
        self.plan = None;
    }
}

pub fn parse_bytes(value: &str) -> Result<Vec<u8>, String> {
    let (value, explicit_hex) = value
        .strip_prefix("hex:")
        .map_or((value, false), |value| (value, true));
    let value = if explicit_hex {
        value.trim().strip_prefix("0x").unwrap_or(value.trim())
    } else {
        value
    };
    if !explicit_hex && value.len() != value.trim().len() {
        return Ok(value.as_bytes().to_vec());
    }
    if explicit_hex || value.starts_with("0x") {
        let hex = if explicit_hex { value } else { &value[2..] };
        if !hex.len().is_multiple_of(2) {
            return Err("hex values require pairs of digits".into());
        }
        return (0..hex.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&hex[index..index + 2], 16)
                    .map_err(|_| "hex values may contain only 0-9, a-f, and A-F".to_owned())
            })
            .collect();
    }
    Ok(value.as_bytes().to_vec())
}

fn parse_lines(value: &str) -> Result<Vec<Vec<u8>>, String> {
    let values = value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_bytes)
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err("at least one value is required".into());
    }
    Ok(values)
}

fn parse_pairs(value: &str) -> Result<Vec<RedisPairDraft>, String> {
    let pairs = value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (left, right) = line
                .split_once('\t')
                .ok_or_else(|| "collection rows require key<TAB>value".to_owned())?;
            Ok((parse_bytes(left)?, parse_bytes(right)?))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if pairs.is_empty() {
        return Err("at least one collection row is required".into());
    }
    Ok(pairs)
}

fn value_page_text(page: &RedisValuePage) -> String {
    match &page.value {
        crate::db::redis::read::RedisPageValue::String(value) => display_bytes(value),
        crate::db::redis::read::RedisPageValue::Hash(values)
        | crate::db::redis::read::RedisPageValue::SortedSet(values) => values
            .iter()
            .map(|(left, right)| format!("{}\t{}", display_bytes(right), display_bytes(left)))
            .collect::<Vec<_>>()
            .join("\n"),
        crate::db::redis::read::RedisPageValue::List(values) => values
            .iter()
            .map(|(_, value)| display_bytes(value))
            .collect::<Vec<_>>()
            .join("\n"),
        crate::db::redis::read::RedisPageValue::Set(values) => values
            .iter()
            .map(|value| display_bytes(value))
            .collect::<Vec<_>>()
            .join("\n"),
        crate::db::redis::read::RedisPageValue::Stream(values) => values
            .iter()
            .flat_map(|(_, fields)| fields.iter())
            .map(|(field, value)| format!("{}\t{}", display_bytes(field), display_bytes(value)))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn display_bytes(value: &[u8]) -> String {
    let printable = value
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ');
    let reserved_prefix = value.starts_with(b"0x") || value.starts_with(b"hex:");
    if printable && !reserved_prefix {
        String::from_utf8_lossy(value).into_owned()
    } else {
        format!(
            "hex:{}",
            value
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }
}
