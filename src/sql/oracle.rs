use std::{borrow::Cow, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OracleSqlError {
    Empty,
    MultipleStatements,
    ClientCommand,
    IncompleteQuote,
    UnsupportedProgram,
}

impl fmt::Display for OracleSqlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "Oracle SQL is empty",
            Self::MultipleStatements => "Oracle execution accepts one SQL statement at a time",
            Self::ClientCommand => "SQL*Plus client commands are not supported",
            Self::IncompleteQuote => "Oracle SQL contains an incomplete quoted value",
            Self::UnsupportedProgram => {
                "PL/SQL blocks are not supported by this execution path yet"
            }
        };
        formatter.write_str(message)
    }
}

/// Prepares one ordinary SQL statement for the Oracle native driver.
///
/// A semicolon is a client-side terminator for ordinary SQL. It must not be
/// sent to the native driver, while semicolons in quoted values and comments
/// must remain untouched.
pub fn prepare_oracle_statement(sql: &str) -> Result<Cow<'_, str>, OracleSqlError> {
    if first_code_byte(sql).is_none() {
        return Err(OracleSqlError::Empty);
    }

    if matches!(first_keyword(sql).as_deref(), Some("BEGIN" | "DECLARE")) {
        return prepare_anonymous_block(sql);
    }
    if matches!(first_keyword(sql).as_deref(), Some("CREATE" | "ALTER")) {
        return Err(OracleSqlError::UnsupportedProgram);
    }

    let mut scanner = Scanner::new(sql);
    let mut terminator = None;
    let mut client_slash = None;
    while let Some(token) = scanner.next_token()? {
        match token {
            Token::Semicolon(index) => {
                if terminator.is_some() {
                    return Err(OracleSqlError::MultipleStatements);
                }
                terminator = Some(index);
            }
            Token::ClientSlash(index) if terminator.is_some() => client_slash = Some(index),
            Token::Code(_) if terminator.is_some() => {
                return Err(OracleSqlError::MultipleStatements);
            }
            Token::ClientSlash(_) => return Err(OracleSqlError::ClientCommand),
            Token::Comment(_) | Token::Quoted(_) | Token::Code(_) => {}
        }
    }

    let Some(terminator) = terminator else {
        return Ok(Cow::Borrowed(sql));
    };
    let mut prepared = String::with_capacity(sql.len().saturating_sub(1));
    prepared.push_str(&sql[..terminator]);
    prepared.push_str(&sql[terminator + 1..client_slash.unwrap_or(sql.len())]);
    if let Some(slash) = client_slash {
        prepared.push_str(&sql[slash + 1..]);
    }
    Ok(Cow::Owned(prepared))
}

pub(crate) fn scan_statement_boundaries(sql: &str) -> Result<Vec<(usize, usize)>, OracleSqlError> {
    let mut boundaries = Vec::new();
    let mut start = 0;
    while start < sql.len() {
        while start < sql.len() && sql.as_bytes()[start].is_ascii_whitespace() {
            start += 1;
        }
        if start >= sql.len() {
            break;
        }
        let remaining = &sql[start..];
        let keyword = first_keyword(remaining).unwrap_or_default();
        if matches!(keyword.as_str(), "BEGIN" | "DECLARE") {
            if remaining.trim().eq_ignore_ascii_case("BEGIN") {
                boundaries.push((start, sql.len()));
                break;
            }
            let end = start + plsql_block_end(remaining)?;
            let mut boundary_end = end;
            let rest = &sql[end..];
            if let Some(slash) = rest
                .char_indices()
                .find(|(_, character)| !character.is_whitespace())
                .map(|(index, _)| index)
                .filter(|index| {
                    rest[*index..].starts_with('/')
                        && rest[*index + 1..]
                            .lines()
                            .next()
                            .is_none_or(|line| line.trim().is_empty())
                })
            {
                boundary_end = end + slash + 1;
            }
            boundaries.push((start, boundary_end));
            start = boundary_end;
        } else {
            let mut scanner = Scanner::new(remaining);
            let mut end = sql.len();
            while let Some(token) = scanner.next_token()? {
                if let Token::Semicolon(index) = token {
                    end = start + index + 1;
                    break;
                }
            }
            boundaries.push((start, end));
            start = end;
        }
    }
    Ok(boundaries)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Token {
    Semicolon(usize),
    Comment(usize),
    Quoted(usize),
    Code(usize),
    ClientSlash(usize),
}

struct Scanner<'a> {
    sql: &'a str,
    index: usize,
    first_word: Option<String>,
}

impl<'a> Scanner<'a> {
    fn new(sql: &'a str) -> Self {
        Self {
            sql,
            index: 0,
            first_word: None,
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, OracleSqlError> {
        let bytes = self.sql.as_bytes();
        while self.index < bytes.len() {
            let index = self.index;
            match bytes[index] {
                byte if byte.is_ascii_whitespace() => self.index += 1,
                b'-' if bytes.get(index + 1) == Some(&b'-') => {
                    self.index = line_comment_end(bytes, index + 2);
                    return Ok(Some(Token::Comment(index)));
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    self.index = block_comment_end(bytes, index + 2)?;
                    return Ok(Some(Token::Comment(index)));
                }
                b'\'' | b'"' => {
                    self.index = quoted_end(bytes, index, bytes[index])?;
                    return Ok(Some(Token::Quoted(index)));
                }
                b'q' | b'Q' | b'n' | b'N' if is_oracle_q_quote(bytes, index) => {
                    self.index = q_quote_end(bytes, index)?;
                    return Ok(Some(Token::Quoted(index)));
                }
                b';' => {
                    self.index += 1;
                    return Ok(Some(Token::Semicolon(index)));
                }
                b'/' if is_client_slash(self.sql, index) => {
                    self.index += 1;
                    return Ok(Some(Token::ClientSlash(index)));
                }
                _ => {
                    let end = code_end(bytes, index);
                    let word = self.sql[index..end].to_ascii_uppercase();
                    if self.first_word.is_none() {
                        self.first_word = Some(word.clone());
                    }
                    self.index = end;
                    return Ok(Some(Token::Code(index)));
                }
            }
        }
        Ok(None)
    }
}

fn first_code_byte(sql: &str) -> Option<usize> {
    let mut scanner = Scanner::new(sql);
    loop {
        match scanner.next_token().ok()? {
            Some(Token::Comment(_)) => {}
            Some(Token::Quoted(index))
            | Some(Token::Code(index))
            | Some(Token::Semicolon(index))
            | Some(Token::ClientSlash(index)) => return Some(index),
            None => return None,
        }
    }
}

fn first_keyword(sql: &str) -> Option<String> {
    let mut scanner = Scanner::new(sql);
    while let Some(token) = scanner.next_token().ok()? {
        if let Token::Code(index) = token {
            let end = code_end(sql.as_bytes(), index);
            return Some(sql[index..end].to_ascii_uppercase());
        }
        if matches!(
            token,
            Token::Quoted(_) | Token::Semicolon(_) | Token::ClientSlash(_)
        ) {
            return None;
        }
    }
    None
}

fn prepare_anonymous_block(sql: &str) -> Result<Cow<'_, str>, OracleSqlError> {
    let end = plsql_block_end(sql)?;
    let trailing = &sql[end..];
    if trailing
        .lines()
        .any(|line| !line.trim().is_empty() && line.trim() != "/")
    {
        return Err(OracleSqlError::MultipleStatements);
    }
    if trailing.lines().any(|line| line.trim() == "/") {
        let mut prepared = sql[..end].to_owned();
        prepared.push_str(
            &trailing
                .lines()
                .filter(|line| line.trim() != "/")
                .collect::<Vec<_>>()
                .join("\n"),
        );
        return Ok(Cow::Owned(prepared));
    }
    Ok(Cow::Borrowed(&sql[..end]))
}

fn plsql_block_end(sql: &str) -> Result<usize, OracleSqlError> {
    let mut scanner = Scanner::new(sql);
    let mut blocks = Vec::new();
    let mut saw_begin = false;
    let mut pending_end = false;
    while let Some(token) = scanner.next_token()? {
        match token {
            Token::Code(index) => {
                let end = code_end(sql.as_bytes(), index);
                let word = sql[index..end].to_ascii_uppercase();
                if pending_end {
                    if matches!(word.as_str(), "IF" | "LOOP" | "CASE") {
                        if blocks.pop() != Some(word.as_str()) {
                            return Err(OracleSqlError::IncompleteQuote);
                        }
                        pending_end = false;
                    }
                    continue;
                }
                match word.as_str() {
                    "BEGIN" => {
                        blocks.push("BEGIN");
                        saw_begin = true;
                    }
                    "IF" if saw_begin => blocks.push("IF"),
                    "LOOP" if saw_begin => blocks.push("LOOP"),
                    "CASE" if saw_begin => blocks.push("CASE"),
                    "END" if saw_begin => pending_end = true,
                    _ => {}
                }
            }
            Token::Semicolon(index) if pending_end => {
                pending_end = false;
                if blocks.pop() != Some("BEGIN") {
                    return Err(OracleSqlError::IncompleteQuote);
                }
                if blocks.is_empty() {
                    return Ok(index + 1);
                }
            }
            _ => {}
        }
    }
    Err(OracleSqlError::IncompleteQuote)
}

fn code_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len()
        && !bytes[index].is_ascii_whitespace()
        && !matches!(bytes[index], b';' | b'\'' | b'"')
    {
        index += 1;
    }
    index.max(1)
}

fn line_comment_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    index
}

fn block_comment_end(bytes: &[u8], mut index: usize) -> Result<usize, OracleSqlError> {
    while index + 1 < bytes.len() {
        if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            return Ok(index + 2);
        }
        index += 1;
    }
    Err(OracleSqlError::IncompleteQuote)
}

fn quoted_end(bytes: &[u8], mut index: usize, quote: u8) -> Result<usize, OracleSqlError> {
    index += 1;
    while index < bytes.len() {
        if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
            } else {
                return Ok(index + 1);
            }
        } else {
            index += 1;
        }
    }
    Err(OracleSqlError::IncompleteQuote)
}

fn is_oracle_q_quote(bytes: &[u8], index: usize) -> bool {
    let quote = if bytes[index] == b'n' || bytes[index] == b'N' {
        bytes.get(index + 1) == Some(&b'q') || bytes.get(index + 1) == Some(&b'Q')
    } else {
        true
    };
    let quote_index = index
        + if quote && (bytes[index] == b'n' || bytes[index] == b'N') {
            2
        } else {
            1
        };
    bytes.get(quote_index) == Some(&b'\'') && bytes.get(quote_index + 1).is_some()
}

fn q_quote_end(bytes: &[u8], index: usize) -> Result<usize, OracleSqlError> {
    let quote_index = index
        + if bytes[index] == b'n' || bytes[index] == b'N' {
            2
        } else {
            1
        };
    let delimiter = bytes
        .get(quote_index + 1)
        .copied()
        .ok_or(OracleSqlError::IncompleteQuote)?;
    let closing = match delimiter {
        b'[' => b']',
        b'(' => b')',
        b'{' => b'}',
        b'<' => b'>',
        value => value,
    };
    let mut cursor = quote_index + 2;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == closing && bytes[cursor + 1] == b'\'' {
            return Ok(cursor + 2);
        }
        cursor += 1;
    }
    Err(OracleSqlError::IncompleteQuote)
}

fn is_client_slash(sql: &str, index: usize) -> bool {
    sql.as_bytes()[index] == b'/'
        && sql[..index].trim_end().ends_with(';')
        && sql[index + 1..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{OracleSqlError, prepare_oracle_statement};

    #[test]
    fn removes_ordinary_sql_terminator() {
        assert_eq!(
            prepare_oracle_statement("SELECT 1 FROM dual;").unwrap(),
            "SELECT 1 FROM dual"
        );
    }

    #[test]
    fn preserves_tail_comment_and_quoted_semicolons() {
        let sql = "SELECT ';', q'[a;b]' FROM dual; -- tail";
        assert_eq!(
            prepare_oracle_statement(sql).unwrap(),
            "SELECT ';', q'[a;b]' FROM dual -- tail"
        );
    }

    #[test]
    fn rejects_multiple_statements_and_empty_input() {
        assert_eq!(
            prepare_oracle_statement("SELECT 1 FROM dual; SELECT 2 FROM dual"),
            Err(OracleSqlError::MultipleStatements)
        );
        assert_eq!(
            prepare_oracle_statement("-- only comment"),
            Err(OracleSqlError::Empty)
        );
    }

    #[test]
    fn rejects_incomplete_quotes_and_accepts_anonymous_blocks() {
        assert_eq!(
            prepare_oracle_statement("SELECT 'x FROM dual"),
            Err(OracleSqlError::IncompleteQuote)
        );
        assert_eq!(
            prepare_oracle_statement("BEGIN NULL; END;").unwrap(),
            "BEGIN NULL; END;"
        );
        assert_eq!(
            prepare_oracle_statement("BEGIN NULL; END;\n/\n").unwrap(),
            "BEGIN NULL; END;"
        );
    }

    #[test]
    fn removes_sqlplus_slash_after_ordinary_sql() {
        assert_eq!(
            prepare_oracle_statement("SELECT 1 FROM dual;\r\n /\r\n").unwrap(),
            "SELECT 1 FROM dual\r\n \r\n"
        );
    }

    #[test]
    fn accepts_nested_plsql_control_blocks() {
        let sql = "BEGIN IF 1 = 1 THEN NULL; END IF; LOOP EXIT; END LOOP; END;";
        assert_eq!(prepare_oracle_statement(sql).unwrap(), sql);
    }
}
