//! Small shared helpers.

/// Percent-encode a string for use in URL path/query segments.
pub fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_spaces_and_plus() {
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("a+b"), "a%2Bb");
        assert_eq!(urlencoding("ok"), "ok");
    }
}
