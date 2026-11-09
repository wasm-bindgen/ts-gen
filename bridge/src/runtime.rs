//! The JS runtime's items the generated glue reaches, so a bridge carries
//! only what it uses.

use std::collections::HashSet;

const RUNTIME: &str = include_str!("runtime.js");

struct Item<'s> {
    name: &'s str,
    src: &'s str,
}

/// The runtime's top-level declarations, each with its leading comment.
fn items() -> Vec<Item<'static>> {
    let mut starts = Vec::new();
    let mut comment = None;
    let mut at = 0;
    for line in RUNTIME.split_inclusive('\n') {
        if line.starts_with("//") {
            comment.get_or_insert(at);
        } else if let Some(name) = declared(line) {
            starts.push((comment.unwrap_or(at), name));
            comment = None;
        } else {
            comment = None;
        }
        at += line.len();
    }
    let ends = starts.iter().skip(1).map(|s| s.0).chain([RUNTIME.len()]);
    starts
        .iter()
        .zip(ends)
        .map(|(&(start, name), end)| Item {
            name,
            src: &RUNTIME[start..end],
        })
        .collect()
}

fn declared(line: &str) -> Option<&str> {
    let rest = ["function ", "class ", "const ", "let "]
        .iter()
        .find_map(|k| line.strip_prefix(k))?;
    let end = rest
        .find(|c: char| !(c == '$' || c == '_' || c.is_ascii_alphanumeric()))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn names(src: &str) -> impl Iterator<Item = &str> {
    src.match_indices('$').map(move |(i, _)| {
        let rest = &src[i..];
        let end = rest[1..]
            .find(|c: char| !(c == '_' || c.is_ascii_alphanumeric()))
            .map_or(rest.len(), |e| e + 1);
        &rest[..end]
    })
}

/// The items `glue` reaches, transitively, in runtime order.
pub(crate) fn used(glue: &str) -> String {
    let items = items();
    let mut reached: HashSet<&str> = HashSet::new();
    let mut todo: Vec<&str> = names(glue).collect();
    while let Some(n) = todo.pop() {
        if let Some(item) = items.iter().find(|i| i.name == n) {
            if reached.insert(item.name) {
                todo.extend(names(item.src));
            }
        }
    }
    items
        .iter()
        .filter(|i| reached.contains(i.name))
        .map(|i| i.src)
        .collect()
}
