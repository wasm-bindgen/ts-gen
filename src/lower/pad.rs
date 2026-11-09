//! A lib type is stubbed with the most type arguments any reference gives
//! it, so the references that give fewer are padded with `Any?`.

use std::collections::HashMap;

use ts_gen_abi::ir as abi;

pub(super) fn type_ref(r: &mut abi::TypeRef, arities: &HashMap<&str, usize>) {
    if let [name] = r.path.segments.as_slice() {
        if let Some(&arity) = arities.get(name.as_str()) {
            while r.args.len() < arity {
                r.args
                    .push(abi::Type::Nullable(Box::new(abi::Type::Any), false));
            }
        }
    }
}
