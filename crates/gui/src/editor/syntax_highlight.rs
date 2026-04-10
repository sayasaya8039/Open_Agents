use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxColorRole {
    Plain,
    Comment,
    Keyword,
    String,
    Number,
    Type,
    Function,
    Property,
    Macro,
    Heading,
    Accent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxSpan {
    pub text: String,
    pub role: SyntaxColorRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    PlainText,
    Rust,
    CFamily,
    Zig,
    Toml,
    Json,
    Markdown,
}

impl Language {
    #[cfg(test)]
    pub fn name(self) -> &'static str {
        match self {
            Self::PlainText => "plain",
            Self::Rust => "rust",
            Self::CFamily => "c-family",
            Self::Zig => "zig",
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Markdown => "markdown",
        }
    }
}

pub fn detect_language(path: Option<&Path>) -> Language {
    let Some(path) = path else {
        return Language::PlainText;
    };

    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some("rs") => Language::Rust,
        Some("c") | Some("h") | Some("cpp") | Some("hpp") | Some("cc") => Language::CFamily,
        Some("zig") => Language::Zig,
        Some("toml") => Language::Toml,
        Some("json") => Language::Json,
        Some("md") | Some("markdown") => Language::Markdown,
        _ => Language::PlainText,
    }
}

pub fn highlight_buffer(path: Option<&Path>, lines: &[String]) -> Vec<Vec<SyntaxSpan>> {
    let language = detect_language(path);
    lines
        .iter()
        .map(|line| highlight_line(language, line))
        .collect()
}

fn highlight_line(language: Language, line: &str) -> Vec<SyntaxSpan> {
    match language {
        Language::Rust => highlight_code_like(
            line,
            &[
                "as", "async", "await", "break", "const", "continue", "crate", "else", "enum",
                "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
                "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
                "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
        ),
        Language::CFamily => highlight_code_like(
            line,
            &[
                "auto", "bool", "break", "case", "char", "const", "continue", "default", "do",
                "double", "else", "enum", "extern", "false", "float", "for", "goto", "if",
                "inline", "int", "long", "register", "restrict", "return", "short", "signed",
                "sizeof", "static", "struct", "switch", "true", "typedef", "union", "unsigned",
                "void", "volatile", "while",
            ],
        ),
        Language::Zig => highlight_code_like(
            line,
            &[
                "addrspace",
                "align",
                "allowzero",
                "and",
                "anyframe",
                "anytype",
                "asm",
                "async",
                "await",
                "break",
                "catch",
                "comptime",
                "const",
                "continue",
                "defer",
                "else",
                "enum",
                "errdefer",
                "error",
                "export",
                "extern",
                "false",
                "fn",
                "for",
                "if",
                "inline",
                "linksection",
                "noalias",
                "null",
                "or",
                "orelse",
                "packed",
                "pub",
                "resume",
                "return",
                "struct",
                "suspend",
                "switch",
                "test",
                "threadlocal",
                "true",
                "try",
                "union",
                "unreachable",
                "usingnamespace",
                "var",
                "volatile",
                "while",
            ],
        ),
        Language::Toml => highlight_toml(line),
        Language::Json => highlight_json(line),
        Language::Markdown => highlight_markdown(line),
        Language::PlainText => vec![SyntaxSpan {
            text: line.to_string(),
            role: SyntaxColorRole::Plain,
        }],
    }
}

fn highlight_code_like(line: &str, keywords: &[&str]) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if starts_with_at(line, i, "//") {
            push_span(&mut spans, &line[i..], SyntaxColorRole::Comment);
            break;
        }

        if starts_with_at(line, i, "/*") {
            if let Some(end) = line[i + 2..].find("*/") {
                let end_ix = i + 2 + end + 2;
                push_span(&mut spans, &line[i..end_ix], SyntaxColorRole::Comment);
                i = end_ix;
                continue;
            }
            push_span(&mut spans, &line[i..], SyntaxColorRole::Comment);
            break;
        }

        let ch = line[i..].chars().next().unwrap();
        let char_len = ch.len_utf8();

        if ch == '"' || ch == '\'' {
            let end_ix = read_string(line, i, ch);
            push_span(&mut spans, &line[i..end_ix], SyntaxColorRole::String);
            i = end_ix;
            continue;
        }

        if ch.is_ascii_digit() {
            let end_ix = read_number(line, i);
            push_span(&mut spans, &line[i..end_ix], SyntaxColorRole::Number);
            i = end_ix;
            continue;
        }

        if is_identifier_start(ch) {
            let end_ix = read_identifier(line, i);
            let ident = &line[i..end_ix];
            let role = classify_code_identifier(line, i, end_ix, ident, keywords);
            push_span(&mut spans, ident, role);
            i = end_ix;
            continue;
        }

        if ch == '@' {
            let end_ix = read_identifier(line, i + char_len);
            push_span(&mut spans, &line[i..end_ix], SyntaxColorRole::Macro);
            i = end_ix;
            continue;
        }

        let role = if "#[](){}<>".contains(ch) {
            SyntaxColorRole::Accent
        } else {
            SyntaxColorRole::Plain
        };
        push_span(&mut spans, &line[i..i + char_len], role);
        i += char_len;
    }

    if spans.is_empty() {
        spans.push(SyntaxSpan {
            text: String::new(),
            role: SyntaxColorRole::Plain,
        });
    }

    spans
}

fn highlight_toml(line: &str) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let trimmed = line.trim_start();

    if trimmed.starts_with('#') {
        return vec![SyntaxSpan {
            text: line.to_string(),
            role: SyntaxColorRole::Comment,
        }];
    }

    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return vec![SyntaxSpan {
            text: line.to_string(),
            role: SyntaxColorRole::Heading,
        }];
    }

    if let Some(eq) = line.find('=') {
        let (left, right) = line.split_at(eq);
        push_span(&mut spans, left, SyntaxColorRole::Property);
        push_span(&mut spans, "=", SyntaxColorRole::Accent);
        let right = &right[1..];
        spans.extend(highlight_value_with_comments(right, '#'));
    } else {
        spans.push(SyntaxSpan {
            text: line.to_string(),
            role: SyntaxColorRole::Plain,
        });
    }

    spans
}

fn highlight_json(line: &str) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let ch = line[i..].chars().next().unwrap();
        let char_len = ch.len_utf8();

        if ch == '"' {
            let end_ix = read_string(line, i, '"');
            let role = if line[end_ix..].trim_start().starts_with(':') {
                SyntaxColorRole::Property
            } else {
                SyntaxColorRole::String
            };
            push_span(&mut spans, &line[i..end_ix], role);
            i = end_ix;
            continue;
        }

        if ch.is_ascii_digit() || ch == '-' {
            let end_ix = read_number(line, i);
            push_span(&mut spans, &line[i..end_ix], SyntaxColorRole::Number);
            i = end_ix;
            continue;
        }

        if is_identifier_start(ch) {
            let end_ix = read_identifier(line, i);
            let ident = &line[i..end_ix];
            let role = match ident {
                "true" | "false" | "null" => SyntaxColorRole::Keyword,
                _ => SyntaxColorRole::Plain,
            };
            push_span(&mut spans, ident, role);
            i = end_ix;
            continue;
        }

        let role = if "{}[]:,".contains(ch) {
            SyntaxColorRole::Accent
        } else {
            SyntaxColorRole::Plain
        };
        push_span(&mut spans, &line[i..i + char_len], role);
        i += char_len;
    }

    spans
}

fn highlight_markdown(line: &str) -> Vec<SyntaxSpan> {
    let trimmed = line.trim_start();
    let role = if trimmed.starts_with("```") || trimmed.starts_with('#') {
        SyntaxColorRole::Heading
    } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("> ") {
        SyntaxColorRole::Accent
    } else {
        SyntaxColorRole::Plain
    };

    vec![SyntaxSpan {
        text: line.to_string(),
        role,
    }]
}

fn highlight_value_with_comments(value: &str, comment_prefix: char) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let value_trimmed = value.trim_start();
    let leading_len = value.len() - value_trimmed.len();

    if leading_len > 0 {
        push_span(&mut spans, &value[..leading_len], SyntaxColorRole::Plain);
    }

    if value_trimmed.is_empty() {
        return spans;
    }

    if value_trimmed.starts_with(comment_prefix) {
        push_span(&mut spans, value_trimmed, SyntaxColorRole::Comment);
        return spans;
    }

    let value_offset = leading_len;
    if value_trimmed.starts_with('"') {
        let end_ix = read_string(value, value_offset, '"');
        push_span(
            &mut spans,
            &value[value_offset..end_ix],
            SyntaxColorRole::String,
        );
        if end_ix < value.len() {
            let rest = &value[end_ix..];
            let comment_at = rest.find(comment_prefix);
            match comment_at {
                Some(idx) => {
                    push_span(&mut spans, &rest[..idx], SyntaxColorRole::Plain);
                    push_span(&mut spans, &rest[idx..], SyntaxColorRole::Comment);
                }
                None => push_span(&mut spans, rest, SyntaxColorRole::Plain),
            }
        }
        return spans;
    }

    let token_end = value[value_offset..]
        .find(comment_prefix)
        .map(|idx| value_offset + idx)
        .unwrap_or(value.len());
    let token = value[value_offset..token_end].trim_end();
    let trailing_ws = &value[value_offset + token.len()..token_end];
    let role = if token.parse::<f64>().is_ok() {
        SyntaxColorRole::Number
    } else if matches!(token, "true" | "false") {
        SyntaxColorRole::Keyword
    } else {
        SyntaxColorRole::Plain
    };
    push_span(&mut spans, token, role);
    push_span(&mut spans, trailing_ws, SyntaxColorRole::Plain);
    if token_end < value.len() {
        push_span(&mut spans, &value[token_end..], SyntaxColorRole::Comment);
    }

    spans
}

fn classify_code_identifier(
    line: &str,
    start: usize,
    end: usize,
    ident: &str,
    keywords: &[&str],
) -> SyntaxColorRole {
    if keywords.contains(&ident) {
        return SyntaxColorRole::Keyword;
    }

    let next_non_ws = line[end..].chars().find(|ch| !ch.is_whitespace());
    let prev_non_ws = line[..start].chars().rev().find(|ch| !ch.is_whitespace());

    if line[end..].starts_with('!') {
        return SyntaxColorRole::Macro;
    }

    if next_non_ws == Some('(') {
        return SyntaxColorRole::Function;
    }

    if prev_non_ws == Some('.') {
        return SyntaxColorRole::Property;
    }

    if ident
        .chars()
        .next()
        .map(|ch| ch.is_ascii_uppercase())
        .unwrap_or(false)
    {
        return SyntaxColorRole::Type;
    }

    SyntaxColorRole::Plain
}

fn read_string(line: &str, start: usize, quote: char) -> usize {
    let bytes = line.as_bytes();
    let mut i = start + quote.len_utf8();
    let mut escaped = false;

    while i < bytes.len() {
        let ch = line[i..].chars().next().unwrap();
        let char_len = ch.len_utf8();
        if escaped {
            escaped = false;
            i += char_len;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            i += char_len;
            continue;
        }
        if ch == quote {
            return i + char_len;
        }
        i += char_len;
    }

    line.len()
}

fn read_number(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start;

    while i < bytes.len() {
        let ch = line[i..].chars().next().unwrap();
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | '+') {
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    i
}

fn read_identifier(line: &str, start: usize) -> usize {
    let bytes = line.as_bytes();
    let mut i = start;

    while i < bytes.len() {
        let ch = line[i..].chars().next().unwrap();
        if is_identifier_part(ch) {
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    i
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_part(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn starts_with_at(line: &str, offset: usize, prefix: &str) -> bool {
    line[offset..].starts_with(prefix)
}

fn push_span(spans: &mut Vec<SyntaxSpan>, text: &str, role: SyntaxColorRole) {
    if text.is_empty() {
        return;
    }

    if let Some(last) = spans.last_mut() {
        if last.role == role {
            last.text.push_str(text);
            return;
        }
    }

    spans.push(SyntaxSpan {
        text: text.to_string(),
        role,
    });
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{detect_language, highlight_buffer, SyntaxColorRole};

    #[test]
    fn detects_rust_files_by_extension() {
        assert_eq!(
            detect_language(Some(Path::new("src/main.rs"))).name(),
            "rust"
        );
    }

    #[test]
    fn highlights_rust_keywords_strings_and_comments() {
        let lines = vec![
            "fn main() {".to_string(),
            "    println!(\"Hello\"); // comment".to_string(),
            "}".to_string(),
        ];

        let highlighted = highlight_buffer(Some(Path::new("src/main.rs")), &lines);

        assert!(highlighted[0]
            .iter()
            .any(|span| span.text == "fn" && span.role == SyntaxColorRole::Keyword));
        assert!(highlighted[1]
            .iter()
            .any(|span| span.text.contains("\"Hello\"") && span.role == SyntaxColorRole::String));
        assert!(highlighted[1]
            .iter()
            .any(|span| span.text.contains("// comment") && span.role == SyntaxColorRole::Comment));
    }
}
