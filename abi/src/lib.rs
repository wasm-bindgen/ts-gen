//! The ts-gen IR and the `tw` bridge ABI, specified in `IR.md`.
//!
//! This crate is everything the producer, the bridge generator and the
//! language generators share, and nothing that depends on TypeScript:
//!
//! * [`ir`]: the model;
//! * [`text`]: the text format;
//! * [`resolve`]: what a type path names, and [`alias`], an IR with its
//!   aliases expanded;
//! * [`key`]: canonical type spelling, member keys and import modules;
//! * [`protocol`]: the `tw` protocol names;
//! * [`profile`]: the profile shared by the bridge and language generators.
//!
//! Built on these, each in its own module: [`validate`] (static checks),
//! [`layout`] (wire layouts), [`naming`] (conflict renaming), [`enums`]
//! (indices and orders) and [`union`] (arm variants), and [`json`], the lowered form for generators
//! not written in Rust.

pub mod alias;
pub mod enums;
pub mod ir;
pub mod json;
pub mod key;
pub mod layout;
pub mod naming;
pub mod profile;
pub mod protocol;
pub mod resolve;
pub mod text;
pub mod union;
pub mod validate;
