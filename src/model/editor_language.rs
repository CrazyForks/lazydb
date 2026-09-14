use crate::sql::SqlDialect;

/// Syntax language for the reusable text viewport. SQL keeps its dialect;
/// data previews deliberately use language-neutral variants.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EditorLanguage {
    Plain,
    Sql(SqlDialect),
    Json,
    Yaml,
}

impl EditorLanguage {
    pub const fn uses_syntax_highlighting(self) -> bool {
        !matches!(self, Self::Plain)
    }
}
