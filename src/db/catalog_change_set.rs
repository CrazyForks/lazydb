//! Database-owned mutation inputs.
//!
//! UI drafts contain focus, validation, and rendering state.  Adapters should
//! eventually consume this smaller representation so fields that were not
//! edited cannot be overwritten with UI defaults.

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldChange<T> {
    Unchanged,
    Changed(T),
    Unknown,
}

impl<T> FieldChange<T> {
    pub fn changed(value: T) -> Self {
        Self::Changed(value)
    }

    pub const fn is_changed(&self) -> bool {
        matches!(self, Self::Changed(_))
    }

    pub fn as_changed(&self) -> Option<&T> {
        match self {
            Self::Changed(value) => Some(value),
            Self::Unchanged | Self::Unknown => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogFieldChanges<T> {
    pub name: FieldChange<String>,
    pub comment: FieldChange<Option<T>>,
}

impl<T> Default for CatalogFieldChanges<T> {
    fn default() -> Self {
        Self {
            name: FieldChange::Unchanged,
            comment: FieldChange::Unchanged,
        }
    }
}

impl<T> CatalogFieldChanges<T> {
    pub fn has_changes(&self) -> bool {
        self.name.is_changed() || self.comment.is_changed()
    }
}
