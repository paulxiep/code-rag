mod python;
mod rust;
mod typescript;

pub use python::PythonHandler;
pub use rust::RustHandler;
pub use typescript::TypeScriptHandler;

use super::language::LanguageHandler;
use std::path::Path;
use std::sync::OnceLock;

/// Global registry of language handlers
static HANDLERS: OnceLock<Vec<Box<dyn LanguageHandler>>> = OnceLock::new();

fn get_handlers() -> &'static Vec<Box<dyn LanguageHandler>> {
    HANDLERS.get_or_init(|| {
        vec![
            Box::new(RustHandler),
            Box::new(PythonHandler),
            Box::new(TypeScriptHandler),
        ]
    })
}

/// Find handler for a file path based on extension
pub fn handler_for_path(path: &Path) -> Option<&'static dyn LanguageHandler> {
    let ext = path.extension()?.to_str()?;
    get_handlers()
        .iter()
        .find(|h| h.extensions().contains(&ext))
        .map(|h| h.as_ref())
}

/// Get handler by language name
pub fn handler_by_name(name: &str) -> Option<&'static dyn LanguageHandler> {
    get_handlers()
        .iter()
        .find(|h| h.name() == name)
        .map(|h| h.as_ref())
}

/// List all supported extensions
pub fn supported_extensions() -> Vec<&'static str> {
    get_handlers()
        .iter()
        .flat_map(|h| h.extensions().iter().copied())
        .collect()
}
