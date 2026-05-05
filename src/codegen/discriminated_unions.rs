//! Codegen for [`crate::ir::DiscriminatedUnionDecl`].
//!
//! A discriminated union is rendered very similarly to a dictionary
//! interface: a single `pub type Foo;` with getter/setter bindings for
//! every property in the merged shape, plus an `impl Foo` block of
//! `new_*` / `builder_*` factories and a `FooBuilder` wrapper carrying
//! the merged-optional fluent setters.
//!
//! The only material difference from the regular dictionary path is the
//! per-branch required-field set used to drive the factory variants.
//! For the merged-shape `Interface` path, a single pass over
//! `optional == false` getters drives all `new_*` variants. For a
//! discriminated union, each branch contributes its own required-field
//! pass, so an `EmailAttachment` whose `inline` branch requires
//! `contentId` and `attachment` branch makes it optional emits
//!
//! ```rust,ignore
//! impl EmailAttachment {
//!     pub fn new_inline(content_id: &str, filename: &str, type_: &str, content: &str) -> Self;
//!     pub fn new_attachment(filename: &str, type_: &str, content: &str) -> Self;
//!     // ...plus the `_with_<type>` value-union variants on each branch.
//! }
//! ```
//!
//! The wrapper `EmailAttachmentBuilder` still exposes a fluent
//! `content_id(self, val)` setter — matching the merged-shape view that
//! the field is optional on the type as a whole — so callers who go
//! through `builder_attachment(...).content_id(x).build()` aren't
//! prevented from setting it. The branch invariant is enforced at the
//! `new_<discriminator>` boundary, not inside the builder.

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::{DiscriminatedUnionDecl, GetterMember, Member, ModuleContext};
use crate::parse::scope::ScopeId;

use super::classes::{generate_dictionary_factory_with_passes, generate_extern_block, ClassConfig};
use super::typemap::CodegenContext;

/// Generate the full `extern` block + factory impls for a
/// [`DiscriminatedUnionDecl`].
pub fn generate_discriminated_union(
    decl: &DiscriminatedUnionDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TokenStream {
    let config = ClassConfig::from_discriminated_union(decl, ctx, cgctx, scope);
    let extern_block = generate_extern_block(&config);

    // For each branch, derive its required-getter list. A property
    // qualifies as required for that branch when:
    //
    // * It's present (some branches may omit a property entirely).
    // * It's non-optional (no `?:` marker) in that branch.
    //
    // We pass the **branch's own getter** rather than the merged-shape
    // getter so the factory pipeline sees the branch-specific type.
    // For example `EmailAttachment.contentId` is `string` in the
    // `inline` branch but `Nullable<String>` in the merged shape; using
    // the branch getter ensures `new_inline(content_id: &str, ...)`
    // takes the narrower type rather than `Option<&str>`.
    //
    // Branch types are typically narrower than merged setter types
    // (the merged type is a LUB across branches). The factory layer
    // handles this by structurally matching the branch type against
    // the merged setter overloads via `pick_setter`; the wrapper-level
    // `.into()` calls inserted in the generated body bridge any
    // remaining gap when the branch type doesn't equal but does
    // convert to the setter param type.
    let required_passes: Vec<Vec<&GetterMember>> = decl
        .branches
        .iter()
        .map(|branch| {
            branch
                .iter()
                .filter_map(|m| match m {
                    Member::Getter(g) if !g.optional => Some(g),
                    _ => None,
                })
                .collect()
        })
        .collect();

    let factory = generate_dictionary_factory_with_passes(&config, Some(required_passes));

    quote! {
        #extern_block
        #factory
    }
}
