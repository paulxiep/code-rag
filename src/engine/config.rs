use super::intent::RoutingTable;

/// RAG pipeline configuration
#[derive(Clone, Debug, Default)]
pub struct EngineConfig {
    pub routing: RoutingTable,
}

/// How many chunks to retrieve
#[derive(Clone, Debug)]
pub struct RetrievalConfig {
    pub code_limit: usize,
    pub readme_limit: usize,
    pub crate_limit: usize,
    pub module_doc_limit: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            code_limit: 5,
            readme_limit: 2,
            crate_limit: 3,
            module_doc_limit: 3,
        }
    }
}
