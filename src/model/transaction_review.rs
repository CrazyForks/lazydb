use uuid::Uuid;

use crate::sql::SqlDialect;

use super::transaction::{DeferredTransactionPrompt, TransactionExitChoice};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TransactionReviewFocus {
    SqlPreview,
    Commit,
    Rollback,
    #[default]
    Cancel,
}

impl TransactionReviewFocus {
    pub fn next(self) -> Self {
        match self {
            Self::SqlPreview => Self::Commit,
            Self::Commit => Self::Rollback,
            Self::Rollback => Self::Cancel,
            Self::Cancel => Self::SqlPreview,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::SqlPreview => Self::Cancel,
            Self::Commit => Self::SqlPreview,
            Self::Rollback => Self::Commit,
            Self::Cancel => Self::Rollback,
        }
    }

    pub fn choice(self) -> Option<TransactionExitChoice> {
        match self {
            Self::SqlPreview => None,
            Self::Commit => Some(TransactionExitChoice::Commit),
            Self::Rollback => Some(TransactionExitChoice::Rollback),
            Self::Cancel => Some(TransactionExitChoice::Cancel),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionReviewState {
    pub tab_id: Uuid,
    pub prompt: Option<DeferredTransactionPrompt>,
    pub focus: TransactionReviewFocus,
    pub editor_session_id: Uuid,
    pub sql: String,
    pub dialect: SqlDialect,
    pub edit_snapshot: Option<String>,
    pub transaction_generation: u64,
}
