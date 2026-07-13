use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn object() -> Self {
        Self::Object(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        let Self::Object(items) = self else {
            return None;
        };
        items
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        let Self::Object(items) = self else {
            return None;
        };
        items
            .iter_mut()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Object(_) => None,
        }
    }

    pub fn entries(&self) -> Option<&[(String, Value)]> {
        match self {
            Self::Object(items) => Some(items),
            Self::Text(_) => None,
        }
    }

    pub fn upsert(&mut self, key: impl Into<String>, value: Value) -> Result<()> {
        let key = key.into();
        let Self::Object(items) = self else {
            bail!("cannot insert into a VDF text value");
        };
        if let Some((_, current)) = items
            .iter_mut()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(&key))
        {
            *current = value;
        } else {
            items.push((key, value));
        }
        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> Result<Option<Value>> {
        let Self::Object(items) = self else {
            bail!("cannot remove from a VDF text value");
        };
        Ok(items
            .iter()
            .position(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|index| items.remove(index).1))
    }

    pub fn ensure_object(&mut self, key: &str) -> Result<&mut Value> {
        let Self::Object(items) = self else {
            bail!("cannot insert into a VDF text value");
        };
        let index = items
            .iter()
            .position(|(candidate, _)| candidate.eq_ignore_ascii_case(key));
        let index = match index {
            Some(index) => index,
            None => {
                items.push((key.to_string(), Value::object()));
                items.len() - 1
            }
        };
        if !matches!(items[index].1, Value::Object(_)) {
            bail!("VDF key {key} is not an object");
        }
        Ok(&mut items[index].1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Text(String),
    Open,
    Close,
}

pub fn parse(input: &str) -> Result<Value> {
    let tokens = lex(input)?;
    let mut cursor = 0;
    let value = parse_object(&tokens, &mut cursor, false)?;
    if cursor != tokens.len() {
        bail!("unexpected trailing VDF token");
    }
    Ok(value)
}

fn parse_object(tokens: &[Token], cursor: &mut usize, expect_close: bool) -> Result<Value> {
    let mut items = Vec::new();
    while *cursor < tokens.len() {
        if matches!(tokens[*cursor], Token::Close) {
            if !expect_close {
                bail!("unexpected closing brace");
            }
            *cursor += 1;
            return Ok(Value::Object(items));
        }
        let Token::Text(key) = &tokens[*cursor] else {
            bail!("expected VDF key");
        };
        let key = key.clone();
        *cursor += 1;
        let token = tokens.get(*cursor).context("missing VDF value")?;
        let value = match token {
            Token::Text(value) => {
                *cursor += 1;
                Value::Text(value.clone())
            }
            Token::Open => {
                *cursor += 1;
                parse_object(tokens, cursor, true)?
            }
            Token::Close => bail!("missing value for VDF key {key}"),
        };
        items.push((key, value));
    }
    if expect_close {
        bail!("unterminated VDF object");
    }
    Ok(Value::Object(items))
}

fn lex(input: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            c if c.is_whitespace() => index += 1,
            '/' if chars.get(index + 1) == Some(&'/') => {
                index += 2;
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '{' => {
                tokens.push(Token::Open);
                index += 1;
            }
            '}' => {
                tokens.push(Token::Close);
                index += 1;
            }
            '"' => {
                let (value, next) = quoted(&chars, index + 1)?;
                tokens.push(Token::Text(value));
                index = next;
            }
            _ => {
                let start = index;
                while index < chars.len()
                    && !chars[index].is_whitespace()
                    && !matches!(chars[index], '{' | '}')
                {
                    index += 1;
                }
                tokens.push(Token::Text(chars[start..index].iter().collect()));
            }
        }
    }
    Ok(tokens)
}

fn quoted(chars: &[char], mut index: usize) -> Result<(String, usize)> {
    let mut value = String::new();
    while index < chars.len() {
        match chars[index] {
            '"' => return Ok((value, index + 1)),
            '\\' => {
                index += 1;
                let escaped = chars.get(index).context("unterminated VDF escape")?;
                value.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    other => *other,
                });
                index += 1;
            }
            ch => {
                value.push(ch);
                index += 1;
            }
        }
    }
    bail!("unterminated VDF string")
}

pub fn to_string(value: &Value) -> Result<String> {
    let Value::Object(items) = value else {
        bail!("VDF root must be an object");
    };
    let mut output = String::new();
    write_items(items, 0, &mut output);
    Ok(output)
}

fn write_items(items: &[(String, Value)], depth: usize, output: &mut String) {
    let indent = "\t".repeat(depth);
    for (key, value) in items {
        output.push_str(&indent);
        output.push('"');
        output.push_str(&escape(key));
        output.push_str("\"\n");
        match value {
            Value::Text(text) => {
                output.push_str(&indent);
                output.push_str("\t\"");
                output.push_str(&escape(text));
                output.push_str("\"\n");
            }
            Value::Object(children) => {
                output.push_str(&indent);
                output.push_str("{\n");
                write_items(children, depth + 1, output);
                output.push_str(&indent);
                output.push_str("}\n");
            }
        }
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_round_trips_nested_objects() {
        let input = r#"
            "AppState"
            {
                "appid" "620"
                "name" "Portal 2"
            }
        "#;
        let parsed = parse(input).unwrap();
        let app = parsed.get("AppState").unwrap();
        assert_eq!(app.get("appid").and_then(Value::text), Some("620"));
        assert_eq!(parse(&to_string(&parsed).unwrap()).unwrap(), parsed);
    }

    #[test]
    fn rejects_unterminated_input() {
        assert!(parse("\"root\" { \"key\" \"value\"").is_err());
    }
}
