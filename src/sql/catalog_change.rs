use sqlparser::{
    ast::{AlterTableOperation, ObjectName, ObjectNamePart, RenameTableNameKind, Statement},
    parser::Parser,
};

use super::{SqlDialect, SqlRisk, dialect::parser_dialect, risk::classify_sql};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogChangeName {
    pub parts: Vec<String>,
}

impl CatalogChangeName {
    pub fn object(&self) -> &str {
        self.parts.last().map(String::as_str).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogChangeKind {
    RenameTable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogChange {
    pub kind: CatalogChangeKind,
    pub old_name: CatalogChangeName,
    pub new_name: CatalogChangeName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogChangeImpact {
    None,
    Changed(Vec<CatalogChange>),
    MultipleStatements { statement_count: usize },
    ParseFailure { message: String },
}

impl CatalogChangeImpact {
    pub fn merge(&mut self, other: Self) {
        if self.is_none() {
            *self = other;
        } else if other.is_none() {
        } else {
            match other {
                Self::Changed(mut other) if matches!(self, Self::Changed(_)) => {
                    if let Self::Changed(changes) = self {
                        changes.append(&mut other);
                    }
                }
                Self::MultipleStatements { statement_count } => {
                    *self = Self::MultipleStatements { statement_count };
                }
                Self::ParseFailure { message } => *self = Self::ParseFailure { message },
                Self::Changed(_) => {
                    *self = Self::ParseFailure {
                        message: "catalog change impact was ambiguous".to_owned(),
                    };
                }
                Self::None => {}
            }
        }
    }

    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

pub fn extract_catalog_change_impact(sql: &str, dialect: SqlDialect) -> CatalogChangeImpact {
    let statements = match Parser::parse_sql(parser_dialect(dialect), sql) {
        Ok(statements) => statements,
        Err(error) => {
            return CatalogChangeImpact::ParseFailure {
                message: error.to_string(),
            };
        }
    };

    if statements.len() != 1 {
        return if statements.is_empty() {
            CatalogChangeImpact::None
        } else {
            CatalogChangeImpact::MultipleStatements {
                statement_count: statements.len(),
            }
        };
    }

    if classify_sql(sql, dialect).risks.as_slice() != [SqlRisk::Ddl] {
        return CatalogChangeImpact::None;
    }

    match &statements[0] {
        Statement::AlterTable(table) => {
            let changes: Vec<_> = table
                .operations
                .iter()
                .filter_map(|operation| match operation {
                    AlterTableOperation::RenameTable { table_name } => {
                        let new_name = match table_name {
                            RenameTableNameKind::As(name) | RenameTableNameKind::To(name) => {
                                CatalogChangeName::from_object_name(name)
                            }
                        };
                        Some(CatalogChange {
                            kind: CatalogChangeKind::RenameTable,
                            old_name: CatalogChangeName::from_object_name(&table.name),
                            new_name,
                        })
                    }
                    _ => None,
                })
                .collect();
            if changes.is_empty() {
                CatalogChangeImpact::None
            } else {
                CatalogChangeImpact::Changed(changes)
            }
        }
        _ => CatalogChangeImpact::None,
    }
}

impl CatalogChangeName {
    fn from_object_name(name: &ObjectName) -> Self {
        Self {
            parts: name
                .0
                .iter()
                .filter_map(|part| match part {
                    ObjectNamePart::Identifier(ident) => Some(ident.value.clone()),
                    ObjectNamePart::Function(_) => None,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename(sql: &str) -> CatalogChange {
        match extract_catalog_change_impact(sql, SqlDialect::Postgres) {
            CatalogChangeImpact::Changed(mut changes) => changes.remove(0),
            impact => panic!("expected rename impact, got {impact:?}"),
        }
    }

    #[test]
    fn extracts_alter_table_rename() {
        assert_eq!(
            rename("ALTER TABLE users RENAME TO accounts"),
            CatalogChange {
                kind: CatalogChangeKind::RenameTable,
                old_name: CatalogChangeName {
                    parts: vec!["users".into()]
                },
                new_name: CatalogChangeName {
                    parts: vec!["accounts".into()]
                },
            }
        );
    }

    #[test]
    fn preserves_quoted_identifiers_without_quote_delimiters() {
        let change = rename("ALTER TABLE \"User Accounts\" RENAME TO \"Customer Accounts\"");

        assert_eq!(change.old_name.object(), "User Accounts");
        assert_eq!(change.new_name.object(), "Customer Accounts");
    }

    #[test]
    fn preserves_schema_qualified_old_name() {
        let change = rename("ALTER TABLE tenant.users RENAME TO accounts");

        assert_eq!(change.old_name.parts, vec!["tenant", "users"]);
        assert_eq!(change.new_name.parts, vec!["accounts"]);
    }

    #[test]
    fn rejects_multi_statement_input_as_ambiguous() {
        assert_eq!(
            extract_catalog_change_impact(
                "ALTER TABLE users RENAME TO accounts; ALTER TABLE orders RENAME TO invoices",
                SqlDialect::Postgres,
            ),
            CatalogChangeImpact::MultipleStatements { statement_count: 2 }
        );
    }

    #[test]
    fn ignores_comments_around_statement() {
        assert!(matches!(
            extract_catalog_change_impact(
                "-- rename the table\nALTER TABLE users RENAME TO accounts /* done */",
                SqlDialect::Postgres,
            ),
            CatalogChangeImpact::Changed(_)
        ));
    }

    #[test]
    fn reports_parse_failure() {
        assert!(matches!(
            extract_catalog_change_impact("ALTER TABLE users RENAME", SqlDialect::Postgres),
            CatalogChangeImpact::ParseFailure { .. }
        ));
    }

    #[test]
    fn ignores_non_ddl_statements() {
        assert_eq!(
            extract_catalog_change_impact("SELECT * FROM users", SqlDialect::Postgres),
            CatalogChangeImpact::None
        );
    }

    #[test]
    fn merges_changes_and_preserves_ambiguity_conservatively() {
        let mut impact = extract_catalog_change_impact(
            "ALTER TABLE users RENAME TO accounts",
            SqlDialect::Postgres,
        );
        impact.merge(extract_catalog_change_impact(
            "ALTER TABLE orders RENAME TO invoices",
            SqlDialect::Postgres,
        ));
        assert!(matches!(impact, CatalogChangeImpact::Changed(ref changes) if changes.len() == 2));

        impact.merge(CatalogChangeImpact::ParseFailure {
            message: "unknown".into(),
        });
        assert!(matches!(impact, CatalogChangeImpact::ParseFailure { .. }));
    }
}
