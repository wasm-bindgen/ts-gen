//! An S-expression reader for the subset of the text format the template
//! uses: lists, atoms (keywords, `$ids`, numbers, `@annotation` names) and
//! strings, with line and block comments skipped.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    List(Vec<Node>),
    Atom(String),
    Str(String),
}

impl Node {
    pub fn atom(&self) -> Option<&str> {
        match self {
            Node::Atom(a) => Some(a),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn list(&self) -> Option<&[Node]> {
        match self {
            Node::List(l) => Some(l),
            _ => None,
        }
    }

    /// The keyword a list starts with: `import` for `(import ...)`,
    /// `@abi.ref` for `(@abi.ref ...)`.
    pub fn head(&self) -> Option<&str> {
        self.list()?.first()?.atom()
    }
}

pub fn read(src: &str) -> Result<Node, String> {
    let mut r = Reader {
        src: src.as_bytes(),
        pos: 0,
    };
    r.skip()?;
    let node = r.node()?;
    r.skip()?;
    if r.pos != r.src.len() {
        return Err(r.error("text after the module"));
    }
    Ok(node)
}

struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn error(&self, msg: &str) -> String {
        let line = self.src[..self.pos].iter().filter(|&&b| b == b'\n').count() + 1;
        format!("line {line}: {msg}")
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// Whitespace and comments.
    fn skip(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b';') if self.src.get(self.pos + 1) == Some(&b';') => {
                    while self.peek().is_some_and(|b| b != b'\n') {
                        self.pos += 1;
                    }
                }
                Some(b'(') if self.src.get(self.pos + 1) == Some(&b';') => {
                    let mut depth = 0usize;
                    loop {
                        match (self.peek(), self.src.get(self.pos + 1)) {
                            (Some(b'('), Some(b';')) => {
                                depth += 1;
                                self.pos += 2;
                            }
                            (Some(b';'), Some(b')')) => {
                                depth -= 1;
                                self.pos += 2;
                                if depth == 0 {
                                    break;
                                }
                            }
                            (Some(_), _) => self.pos += 1,
                            (None, _) => return Err(self.error("unterminated block comment")),
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn node(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some(b'(') => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip()?;
                    match self.peek() {
                        Some(b')') => {
                            self.pos += 1;
                            return Ok(Node::List(items));
                        }
                        None => return Err(self.error("unclosed list")),
                        _ => items.push(self.node()?),
                    }
                }
            }
            Some(b')') => Err(self.error("unexpected `)`")),
            Some(b'"') => self.string(),
            Some(_) => {
                let start = self.pos;
                while self
                    .peek()
                    .is_some_and(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'"' | b';'))
                {
                    self.pos += 1;
                }
                Ok(Node::Atom(
                    String::from_utf8_lossy(&self.src[start..self.pos]).into_owned(),
                ))
            }
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn string(&mut self) -> Result<Node, String> {
        self.pos += 1;
        let mut bytes = Vec::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            self.pos += 1;
            match b {
                b'"' => break,
                b'\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.error("unterminated escape"));
                    };
                    self.pos += 1;
                    match e {
                        b'n' => bytes.push(b'\n'),
                        b't' => bytes.push(b'\t'),
                        b'r' => bytes.push(b'\r'),
                        b'"' => bytes.push(b'"'),
                        b'\'' => bytes.push(b'\''),
                        b'\\' => bytes.push(b'\\'),
                        b'u' => {
                            if self.peek() != Some(b'{') {
                                return Err(self.error("expected `{` after `\\u`"));
                            }
                            let close = self.src[self.pos..]
                                .iter()
                                .position(|&b| b == b'}')
                                .ok_or_else(|| self.error("unterminated `\\u{`"))?;
                            let hex = std::str::from_utf8(&self.src[self.pos + 1..self.pos + close])
                                .map_err(|_| self.error("bad `\\u` escape"))?;
                            let c = u32::from_str_radix(&hex.replace('_', ""), 16)
                                .ok()
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.error("bad `\\u` escape"))?;
                            self.pos += close + 1;
                            let mut buf = [0; 4];
                            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        }
                        h => {
                            let l = self.peek().ok_or_else(|| self.error("bad escape"))?;
                            self.pos += 1;
                            let hex = [h, l];
                            let hex = std::str::from_utf8(&hex).map_err(|_| self.error("bad escape"))?;
                            bytes.push(
                                u8::from_str_radix(hex, 16).map_err(|_| self.error("bad escape"))?,
                            );
                        }
                    }
                }
                b => bytes.push(b),
            }
        }
        String::from_utf8(bytes)
            .map(Node::Str)
            .map_err(|_| self.error("string is not UTF-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lists_atoms_strings_and_skips_comments() {
        let n = read("(module ;; c\n (; block (; nested ;) ;) (@abi.ref \"a\\\"b\\u{e9}\\41\" null))")
            .unwrap();
        assert_eq!(
            n,
            Node::List(vec![
                Node::Atom("module".into()),
                Node::List(vec![
                    Node::Atom("@abi.ref".into()),
                    Node::Str("a\"béA".into()),
                    Node::Atom("null".into()),
                ]),
            ])
        );
    }
}
