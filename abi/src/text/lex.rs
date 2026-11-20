use super::Error;
use crate::ir::Span;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Tok {
    Ident(String),
    Str(String),
    Int(i64),
    Doc(String),
    P(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// Longest first.
const PUNCT: [&str; 20] = [
    "...", "->", "=>", "{", "}", "(", ")", "[", "]", "<", ">", ",", ";", ":", ".", "|", "=", "?",
    "@", "*",
];

pub(super) fn lex(src: &str) -> Result<Vec<Token>, Error> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    let at = |i: usize| chars.get(i).copied();
    let err = |line, col, msg: &str| Error::new(Span { line, col }, msg);
    while i < chars.len() {
        let c = chars[i];
        let (tl, tc) = (line, col);
        let start = i;
        let tok = if c == '\n' {
            i += 1;
            line += 1;
            col = 1;
            continue;
        } else if c.is_whitespace() {
            i += 1;
            col += 1;
            continue;
        } else if c == '/' && at(i + 1) == Some('/') {
            let end = chars[i..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(chars.len(), |n| i + n);
            let text: String = chars[i..end].iter().collect();
            col += (end - i) as u32;
            i = end;
            match text.strip_prefix("///") {
                Some(rest) if !rest.starts_with('/') => {
                    Tok::Doc(rest.strip_prefix(' ').unwrap_or(rest).into())
                }
                _ => continue,
            }
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                let Some(c) = at(i) else {
                    return Err(err(tl, tc, "unterminated string"));
                };
                i += 1;
                match c {
                    '"' => break,
                    '\n' => return Err(err(tl, tc, "unterminated string")),
                    '\\' => {
                        let e = at(i);
                        i += 1;
                        match e {
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some('n') => s.push('\n'),
                            Some('r') => s.push('\r'),
                            Some('t') => s.push('\t'),
                            Some('u') if at(i) == Some('{') => {
                                let end = chars[i..].iter().position(|&c| c == '}');
                                let code = end
                                    .map(|n| chars[i + 1..i + n].iter().collect::<String>())
                                    .and_then(|h| u32::from_str_radix(&h, 16).ok())
                                    .and_then(char::from_u32);
                                match (code, end) {
                                    (Some(c), Some(n)) => {
                                        s.push(c);
                                        i += n + 1;
                                    }
                                    _ => {
                                        let col = tc + (i - start) as u32 - 2;
                                        return Err(err(tl, col, "bad `\\u{...}` escape"));
                                    }
                                }
                            }
                            _ => {
                                let col = tc + (i - start) as u32 - 2;
                                return Err(err(tl, col, "unknown escape"));
                            }
                        }
                    }
                    c => s.push(c),
                }
            }
            col += (i - start) as u32;
            Tok::Str(s)
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            while at(i).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                i += 1;
            }
            col += (i - start) as u32;
            Tok::Ident(chars[start..i].iter().collect())
        } else if c.is_ascii_digit() || (c == '-' && at(i + 1).is_some_and(|c| c.is_ascii_digit()))
        {
            i += 1;
            while at(i).is_some_and(|c| c.is_ascii_digit()) {
                i += 1;
            }
            col += (i - start) as u32;
            let text: String = chars[start..i].iter().collect();
            match text.parse() {
                Ok(n) => Tok::Int(n),
                Err(_) => return Err(err(tl, tc, "integer literal out of range")),
            }
        } else {
            let p = PUNCT
                .iter()
                .find(|p| p.chars().enumerate().all(|(k, pc)| at(i + k) == Some(pc)))
                .ok_or_else(|| err(tl, tc, &format!("unexpected character `{c}`")))?;
            i += p.len();
            col += p.len() as u32;
            Tok::P(p)
        };
        toks.push(Token {
            tok,
            span: Span { line: tl, col: tc },
        });
    }
    toks.push(Token {
        tok: Tok::Eof,
        span: Span { line, col },
    });
    Ok(toks)
}
