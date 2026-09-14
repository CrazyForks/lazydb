pub fn display_bytes(value: &[u8]) -> String {
    let mut output = String::new();
    let mut index = 0;
    while index < value.len() {
        if let Ok(text) = std::str::from_utf8(&value[index..]) {
            for character in text.chars() {
                if character.is_control() {
                    output.push_str(&format!("\\x{:02x}", character as u32));
                } else {
                    output.push(character);
                }
            }
            break;
        }
        let byte = value[index];
        if byte.is_ascii_graphic() || byte == b' ' {
            output.push(byte as char);
        } else {
            output.push_str(&format!("\\x{byte:02x}"));
        }
        index += 1;
    }
    output
}

pub fn clipboard_text(value: &[u8]) -> (String, bool) {
    match std::str::from_utf8(value) {
        Ok(text) => (text.to_owned(), false),
        Err(_) => (display_bytes(value), true),
    }
}

#[cfg(test)]
mod tests {
    use super::{clipboard_text, display_bytes};

    #[test]
    fn preserves_utf8_and_escapes_non_printable_bytes() {
        assert_eq!(display_bytes(b"user:1"), "user:1");
        assert_eq!(display_bytes(b"a\n\xff"), "a\\x0a\\xff");
        assert_eq!(display_bytes("用户".as_bytes()), "用户");
    }

    #[test]
    fn marks_binary_clipboard_text_as_escaped() {
        assert_eq!(clipboard_text("用户".as_bytes()), ("用户".into(), false));
        assert_eq!(clipboard_text(&[b'a', 0xff]), ("a\\xff".into(), true));
    }
}
