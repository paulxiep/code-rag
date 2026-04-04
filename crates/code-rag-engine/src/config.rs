use super::intent::RoutingTable;

/// RAG pipeline configuration
#[derive(Clone, Debug, Default)]
pub struct EngineConfig {
    pub routing: RoutingTable,
    pub rerank: RerankConfig,
    pub hybrid: HybridConfig,
}

/// Hybrid search (BM25 + semantic) configuration.
#[derive(Clone, Debug)]
pub struct HybridConfig {
    /// Whether hybrid search is enabled.
    /// When false, only vector search is used (pre-B2 behavior).
    pub enabled: bool,
    /// RRF k parameter. Higher k = more equal weighting between sources.
    /// Standard value: 60.0. Lower values favor top-ranked results more.
    pub rrf_k: f32,
}

impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rrf_k: 60.0,
        }
    }
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

/// Cross-encoder reranking configuration.
#[derive(Clone, Debug)]
pub struct RerankConfig {
    /// Whether reranking is enabled.
    pub enabled: bool,
    /// Per-type over-retrieval multipliers. fetch_limit = final_limit * multiplier.
    pub code_fetch_multiplier: usize,
    pub readme_fetch_multiplier: usize,
    pub crate_fetch_multiplier: usize,
    pub module_doc_fetch_multiplier: usize,
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            code_fetch_multiplier: 4,
            readme_fetch_multiplier: 2,
            crate_fetch_multiplier: 1,
            module_doc_fetch_multiplier: 2,
        }
    }
}

/// Compute fetch limits by applying per-type rerank multipliers to final limits.
pub fn fetch_limits(final_config: &RetrievalConfig, rerank: &RerankConfig) -> RetrievalConfig {
    if !rerank.enabled {
        return final_config.clone();
    }
    RetrievalConfig {
        code_limit: final_config.code_limit * rerank.code_fetch_multiplier,
        readme_limit: final_config.readme_limit * rerank.readme_fetch_multiplier,
        crate_limit: final_config.crate_limit * rerank.crate_fetch_multiplier,
        module_doc_limit: final_config.module_doc_limit * rerank.module_doc_fetch_multiplier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_limits_per_type() {
        let config = RetrievalConfig {
            code_limit: 5,
            readme_limit: 2,
            crate_limit: 3,
            module_doc_limit: 3,
        };
        let rerank = RerankConfig {
            enabled: true,
            code_fetch_multiplier: 4,
            readme_fetch_multiplier: 2,
            crate_fetch_multiplier: 1,
            module_doc_fetch_multiplier: 2,
        };
        let fetched = fetch_limits(&config, &rerank);
        assert_eq!(fetched.code_limit, 20);
        assert_eq!(fetched.readme_limit, 4);
        assert_eq!(fetched.crate_limit, 3);
        assert_eq!(fetched.module_doc_limit, 6);
    }

    #[test]
    fn test_fetch_limits_disabled() {
        let config = RetrievalConfig {
            code_limit: 5,
            readme_limit: 2,
            crate_limit: 3,
            module_doc_limit: 3,
        };
        let rerank = RerankConfig {
            enabled: false,
            ..Default::default()
        };
        let fetched = fetch_limits(&config, &rerank);
        assert_eq!(fetched.code_limit, 5);
        assert_eq!(fetched.readme_limit, 2);
        assert_eq!(fetched.crate_limit, 3);
        assert_eq!(fetched.module_doc_limit, 3);
    }

    #[test]
    fn test_rerank_config_default() {
        let rc = RerankConfig::default();
        assert!(!rc.enabled);
        assert_eq!(rc.code_fetch_multiplier, 4);
        assert_eq!(rc.crate_fetch_multiplier, 1);
    }

    #[test]
    fn test_hybrid_config_default() {
        let hc = HybridConfig::default();
        assert!(!hc.enabled);
        assert!((hc.rrf_k - 60.0).abs() < f32::EPSILON);
    }
}
