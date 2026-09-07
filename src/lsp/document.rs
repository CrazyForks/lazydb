use std::collections::HashMap;

use tower_lsp_server::ls_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    TextDocumentContentChangeEvent, Uri,
};

use super::position::PositionIndex;

#[derive(Clone, Debug)]
pub struct Document {
    pub uri: Uri,
    pub language_id: String,
    pub version: i32,
    pub text: String,
}

impl Document {
    fn from_open(params: DidOpenTextDocumentParams) -> Self {
        Self {
            uri: params.text_document.uri,
            language_id: params.text_document.language_id,
            version: params.text_document.version,
            text: params.text_document.text,
        }
    }

    fn apply_full_change(&mut self, version: i32, change: &TextDocumentContentChangeEvent) {
        self.version = version;
        self.text.clone_from(&change.text);
    }

    fn apply_change(&mut self, version: i32, change: &TextDocumentContentChangeEvent) -> bool {
        let Some(range) = change.range else {
            self.apply_full_change(version, change);
            return true;
        };
        let index = PositionIndex::new(self.text.clone());
        let start = index.offset(range.start);
        let end = index.offset(range.end);
        if start > end || end > self.text.len() || !self.text.is_char_boundary(start) {
            return false;
        }
        let end = end.min(self.text.len());
        if !self.text.is_char_boundary(end) {
            return false;
        }
        self.text.replace_range(start..end, &change.text);
        self.version = version;
        true
    }
}

#[derive(Debug, Default)]
pub struct Documents {
    entries: HashMap<Uri, Document>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::{
        Position, Range, TextDocumentContentChangeEvent, TextDocumentItem,
        VersionedTextDocumentIdentifier,
    };

    fn uri() -> Uri {
        "file:///tmp/query.sql".parse().expect("valid test URI")
    }

    fn opened(version: i32, text: &str) -> DidOpenTextDocumentParams {
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri(),
                language_id: "sql".into(),
                version,
                text: text.into(),
            },
        }
    }

    #[test]
    fn stale_changes_are_ignored_and_full_changes_replace_text() {
        let mut documents = Documents::default();
        documents.open(opened(3, "select 1"));

        let stale = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "stale".into(),
            }],
        };
        assert!(documents.change(stale).is_none());
        assert_eq!(documents.get(&uri()).expect("document").text, "select 1");

        let current = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(),
                version: 4,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "select 2".into(),
            }],
        };
        assert_eq!(documents.change(current).expect("change").text, "select 2");
    }

    #[test]
    fn ranged_changes_use_utf16_positions() {
        let mut documents = Documents::default();
        documents.open(opened(1, "😀 name"));
        let changed = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 3), Position::new(0, 7))),
                range_length: None,
                text: "id".into(),
            }],
        };
        assert_eq!(documents.change(changed).expect("change").text, "😀 id");
    }

    #[test]
    fn close_removes_document() {
        let mut documents = Documents::default();
        documents.open(opened(1, "select 1"));
        assert!(documents.close(&uri()).is_some());
        assert!(documents.get(&uri()).is_none());
    }
}

impl Documents {
    pub fn open(&mut self, params: DidOpenTextDocumentParams) -> Document {
        let document = Document::from_open(params);
        self.entries.insert(document.uri.clone(), document.clone());
        document
    }

    pub fn change(&mut self, params: DidChangeTextDocumentParams) -> Option<Document> {
        let uri = params.text_document.uri;
        let document = self.entries.get_mut(&uri)?;
        if params.text_document.version <= document.version {
            return None;
        }
        for change in &params.content_changes {
            if !document.apply_change(params.text_document.version, change) {
                return None;
            }
        }
        Some(document.clone())
    }

    pub fn save(&mut self, params: DidSaveTextDocumentParams) -> Option<Document> {
        self.entries.get(&params.text_document.uri).cloned()
    }

    pub fn close(&mut self, uri: &Uri) -> Option<Document> {
        self.entries.remove(uri)
    }

    pub fn get(&self, uri: &Uri) -> Option<&Document> {
        self.entries.get(uri)
    }
}
