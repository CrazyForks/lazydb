use std::collections::BTreeMap;

use uuid::Uuid;

use super::{execution_target::ExecutionTarget, transaction::TransactionMode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleDocument {
    pub id: Uuid,
    pub name: String,
    pub execution_target: Option<ExecutionTarget>,
    pub transaction_mode: TransactionMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConsoleDocumentError {
    DuplicateId(Uuid),
    DuplicateName(String),
    EmptyName,
    ControlCharacter,
    MissingDocument(Uuid),
}

impl std::fmt::Display for ConsoleDocumentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(formatter, "duplicate console ID {id}"),
            Self::DuplicateName(name) => write!(formatter, "console name already exists: {name}"),
            Self::EmptyName => formatter.write_str("console name is required"),
            Self::ControlCharacter => {
                formatter.write_str("console name contains a control character")
            }
            Self::MissingDocument(id) => write!(formatter, "console document does not exist: {id}"),
        }
    }
}

impl std::error::Error for ConsoleDocumentError {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConsoleDocuments {
    entries: BTreeMap<Uuid, ConsoleDocument>,
}

impl ConsoleDocuments {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: Uuid) -> Option<&ConsoleDocument> {
        self.entries.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ConsoleDocument> {
        self.entries.values()
    }

    pub fn insert(&mut self, document: ConsoleDocument) -> Result<(), ConsoleDocumentError> {
        let mut document = document;
        document.name = normalize_name(document.name);
        validate_name(&document.name)?;
        if self.entries.contains_key(&document.id) {
            return Err(ConsoleDocumentError::DuplicateId(document.id));
        }
        if self.name_taken(&document.name, None) {
            return Err(ConsoleDocumentError::DuplicateName(document.name));
        }
        self.entries.insert(document.id, document);
        Ok(())
    }

    pub fn rename(
        &mut self,
        id: Uuid,
        name: impl Into<String>,
    ) -> Result<(), ConsoleDocumentError> {
        let name = normalize_name(name.into());
        validate_name(&name)?;
        if !self.entries.contains_key(&id) {
            return Err(ConsoleDocumentError::MissingDocument(id));
        }
        if self.name_taken(&name, Some(id)) {
            return Err(ConsoleDocumentError::DuplicateName(name));
        }
        self.entries
            .get_mut(&id)
            .expect("document existence checked above")
            .name = name;
        Ok(())
    }

    pub fn remove(&mut self, id: Uuid) -> Option<ConsoleDocument> {
        self.entries.remove(&id)
    }

    pub fn next_name(&self) -> String {
        (1..)
            .map(|number| format!("console_{number}"))
            .find(|name| !self.name_taken(name, None))
            .expect("console name sequence must not exhaust usize")
    }

    fn name_taken(&self, name: &str, except: Option<Uuid>) -> bool {
        let normalized = name.trim().to_ascii_lowercase();
        self.entries.values().any(|document| {
            Some(document.id) != except && document.name.trim().to_ascii_lowercase() == normalized
        })
    }
}

pub fn normalize_name(name: String) -> String {
    name.trim().to_owned()
}

pub fn validate_name(name: &str) -> Result<(), ConsoleDocumentError> {
    if name.is_empty() {
        return Err(ConsoleDocumentError::EmptyName);
    }
    if name.chars().any(char::is_control) {
        return Err(ConsoleDocumentError::ControlCharacter);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: u128, name: &str) -> ConsoleDocument {
        ConsoleDocument {
            id: Uuid::from_u128(id),
            name: name.into(),
            execution_target: None,
            transaction_mode: TransactionMode::Auto,
        }
    }

    #[test]
    fn rejects_duplicate_ids_and_case_insensitive_names() {
        let mut documents = ConsoleDocuments::new();
        documents.insert(document(1, "console")).unwrap();
        assert_eq!(
            documents.insert(document(1, "other")),
            Err(ConsoleDocumentError::DuplicateId(Uuid::from_u128(1)))
        );
        assert_eq!(
            documents.insert(document(2, " CONSOLE ")),
            Err(ConsoleDocumentError::DuplicateName("CONSOLE".into()))
        );
    }

    #[test]
    fn next_name_includes_closed_documents_and_reuses_deleted_names() {
        let mut documents = ConsoleDocuments::new();
        documents.insert(document(1, "console_1")).unwrap();
        documents.insert(document(2, "console_3")).unwrap();
        assert_eq!(documents.next_name(), "console_2");
        documents.remove(Uuid::from_u128(1));
        assert_eq!(documents.next_name(), "console_1");
    }

    #[test]
    fn rename_trims_input_and_keeps_same_name_as_a_noop() {
        let mut documents = ConsoleDocuments::new();
        documents.insert(document(1, "console")).unwrap();
        assert!(documents.rename(Uuid::from_u128(1), " console ").is_ok());
        assert_eq!(documents.get(Uuid::from_u128(1)).unwrap().name, "console");
    }

    #[test]
    fn rejects_empty_and_control_character_names() {
        let mut documents = ConsoleDocuments::new();
        assert_eq!(
            documents.insert(document(1, "  ")),
            Err(ConsoleDocumentError::EmptyName)
        );
        assert_eq!(
            documents.insert(document(2, "bad\nname")),
            Err(ConsoleDocumentError::ControlCharacter)
        );
    }
}
