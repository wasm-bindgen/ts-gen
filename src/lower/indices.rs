//! Implicit indices of two unrelated enums always overlap, so enums
//! combined as the bases of one (`extends A, B`) are numbered apart (IR.md
//! §8). They're numbered in declaration order, bases first, and an index is
//! stated only where it differs from the implicit one.

use std::collections::{HashMap, HashSet};

use ts_gen_abi::ir as abi;
use ts_gen_abi::resolve::{Context, Location, Symbols, Target, TypeDecl};

pub(super) fn number(ir: &mut abi::Ir) {
    let stated = plan(ir);
    if stated.is_empty() {
        return;
    }
    enums(&mut ir.items, None, &mut Vec::new(), &mut |loc, e| {
        for a in &mut e.arms {
            if let Some(&n) = stated.get(&(loc.clone(), a.name.clone())) {
                a.value = Some(abi::ArmValue::Int(n));
            }
        }
    });
}

/// The stated index of each arm that needs one.
fn plan(ir: &abi::Ir) -> HashMap<(Location, String), i64> {
    let symbols = Symbols::new(ir);
    let mut all: Vec<(Location, &abi::Enum)> = Vec::new();
    collect(&ir.items, None, &mut Vec::new(), &mut all);
    let decls: HashMap<&Location, &abi::Enum> = all.iter().map(|(l, e)| (l, *e)).collect();
    // A base outside this IR, `err` or a carrier enum keeps its indices, so
    // an enum over one is left alone.
    let bases = |loc: &Location, e: &abi::Enum| -> Option<Vec<Location>> {
        let cx = Context {
            module: loc.module.as_deref(),
            namespace: loc.path[..loc.path.len() - 1]
                .iter()
                .map(String::as_str)
                .collect(),
            ..Context::default()
        };
        e.extends
            .iter()
            .map(|p| match symbols.resolve(p, &cx)? {
                Target::Decl(l, TypeDecl::Enum(b)) if b.carrier.is_none() => Some(l.clone()),
                _ => None,
            })
            .collect()
    };
    let mut combined: HashSet<Location> = HashSet::new();
    let mut stack: Vec<Location> = Vec::new();
    for (loc, e) in &all {
        if e.extends.len() < 2 || e.carrier.is_some() {
            continue;
        }
        if let Some(bs) = bases(loc, e) {
            stack.extend(bs);
        }
    }
    while let Some(l) = stack.pop() {
        if combined.insert(l.clone()) {
            if let Some(bs) = decls.get(&l).and_then(|e| bases(&l, e)) {
                stack.extend(bs);
            }
        }
    }

    let mut out = HashMap::new();
    let mut highest: HashMap<Location, i64> = HashMap::new();
    let mut next = 1;
    let mut order: Vec<Location> = Vec::new();
    fn topo(
        l: &Location,
        combined: &HashSet<Location>,
        bases: &dyn Fn(&Location) -> Vec<Location>,
        order: &mut Vec<Location>,
        visiting: &mut Vec<Location>,
    ) {
        if !combined.contains(l) || order.contains(l) || visiting.contains(l) {
            return;
        }
        visiting.push(l.clone());
        for b in bases(l) {
            topo(&b, combined, bases, order, visiting);
        }
        visiting.pop();
        order.push(l.clone());
    }
    let direct = |l: &Location| decls.get(l).and_then(|e| bases(l, e)).unwrap_or_default();
    for (loc, _) in &all {
        topo(loc, &combined, &direct, &mut order, &mut Vec::new());
    }
    for loc in order {
        let e = decls[&loc];
        let mut before = direct(&loc)
            .iter()
            .filter_map(|b| highest.get(b).copied())
            .max()
            .unwrap_or(0);
        for a in &e.arms {
            if a.meta.attrs.fallback || matches!(a.value, Some(abi::ArmValue::Alias(_))) {
                continue;
            }
            if next != before + 1 {
                out.insert((loc.clone(), a.name.clone()), next);
            }
            before = next;
            next += 1;
        }
        highest.insert(loc, before);
    }
    out
}

fn collect<'a>(
    items: &'a [abi::Decl],
    module: Option<&str>,
    ns: &mut Vec<String>,
    out: &mut Vec<(Location, &'a abi::Enum)>,
) {
    for d in items {
        match d {
            abi::Decl::Namespace(n) => {
                ns.push(n.name.clone());
                collect(&n.items, module, ns, out);
                ns.pop();
            }
            abi::Decl::Module(m) => collect(&m.items, Some(&m.spec), &mut Vec::new(), out),
            abi::Decl::Enum(e) => {
                let mut path = ns.clone();
                path.push(e.name.clone());
                out.push((
                    Location {
                        module: module.map(str::to_string),
                        path,
                    },
                    e,
                ));
            }
            abi::Decl::Class(_) | abi::Decl::Member(_) | abi::Decl::Alias(_) => {}
        }
    }
}

fn enums(
    items: &mut [abi::Decl],
    module: Option<&str>,
    ns: &mut Vec<String>,
    f: &mut impl FnMut(&Location, &mut abi::Enum),
) {
    for d in items {
        match d {
            abi::Decl::Namespace(n) => {
                ns.push(n.name.clone());
                enums(&mut n.items, module, ns, f);
                ns.pop();
            }
            abi::Decl::Module(m) => {
                let spec = m.spec.clone();
                enums(&mut m.items, Some(&spec), &mut Vec::new(), f)
            }
            abi::Decl::Enum(e) => {
                let mut path = ns.clone();
                path.push(e.name.clone());
                let loc = Location {
                    module: module.map(str::to_string),
                    path,
                };
                f(&loc, e)
            }
            abi::Decl::Class(_) | abi::Decl::Member(_) | abi::Decl::Alias(_) => {}
        }
    }
}
