//! Concrete `LlmClient` seam impls for the Caravan-flipped LLM seam.
//!
//! M5: extracted from `code-rag-chat`'s `src/engine/generator.rs` so the
//! impl lives in a library crate. This lets a Caravan-emitted synthetic
//! peer service build a binary that depends on `code-rag-llm` (a library)
//! rather than `code-rag-chat` (the host binary, unbuildable as a
//! library). Same shape applies if more providers are added: each lands
//! as a sibling module here.

pub mod rig_gemini;

pub use rig_gemini::RigGeminiImpl;
