//! code-rag-engine: Platform-agnostic RAG pipeline algorithms
//!
//! Pure functions for intent classification, context building, retrieval scoring,
//! and query routing. No I/O, no HTTP, no filesystem — compiles to both native
//! and wasm32.

pub mod config;
pub mod context;
pub mod fusion;
pub mod intent;
pub mod retriever;
