//! Shared text primitives: tokenization, IDF tables, BM25 scoring,
//! searchable_text construction.
//!
//! Pure (no I/O, no tokio, no fastembed). Compiles to native + wasm32.
//! Single source of truth — no duplicates elsewhere in the workspace.

pub mod bm25;
mod idf;
mod searchable;
mod tokenize;

pub use bm25::{Bm25Params, score};
pub use idf::IdfTable;
pub use searchable::{build_searchable_text, split_camel_case};
pub use tokenize::tokenize;
