use super::intent::RoutingTable;

/// RAG pipeline configuration
#[derive(Clone, Debug, Default)]
pub struct EngineConfig {
    pub routing: RoutingTable,
    pub rerank: RerankConfig,
    pub hybrid: HybridConfig,
    pub dual_embedding: DualEmbeddingConfig,
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
            enabled: true, // B3: searchable_text + signatures fix B2 regression (harness verified)
            rrf_k: 60.0,
        }
    }
}

/// Dual-embedding (signature_vector + body_vector) configuration.
/// When enabled, the CODE table exposes a second vector column derived from
/// signature text, searched in parallel with the body vector and fused via RRF.
/// The per-intent arm policy ultimately decides whether sig-vec is used.
#[derive(Clone, Debug, Default)]
pub struct DualEmbeddingConfig {
    /// Whether dual-embedding retrieval is enabled.
    /// When false, only the body vector column is queried (pre-B5 behavior).
    pub enabled: bool,
}

/// How many chunks to retrieve
#[derive(Clone, Debug)]
pub struct RetrievalConfig {
    pub code_limit: usize,
    pub readme_limit: usize,
    pub crate_limit: usize,
    pub module_doc_limit: usize,
    /// A2: folder-level summary chunks. Default 0 — the arm is wired end-to-end
    /// but returns no chunks until A3 flips the per-intent route limits.
    pub folder_limit: usize,
    /// A4: file-level summary chunks. Default 0 — the RoutingTable supplies
    /// per-intent values. Instantiating RetrievalConfig by hand gets a
    /// zero-risk fallback (arm short-circuits on limit==0).
    pub file_limit: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            code_limit: 5,
            readme_limit: 2,
            crate_limit: 3,
            module_doc_limit: 3,
            folder_limit: 0,
            file_limit: 0,
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
    pub folder_fetch_multiplier: usize,
    /// A4: multiplier for file-level chunks.
    pub file_fetch_multiplier: usize,
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            code_fetch_multiplier: 4,
            readme_fetch_multiplier: 2,
            crate_fetch_multiplier: 1,
            module_doc_fetch_multiplier: 2,
            folder_fetch_multiplier: 2,
            file_fetch_multiplier: 2,
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
        folder_limit: final_config.folder_limit * rerank.folder_fetch_multiplier,
        file_limit: final_config.file_limit * rerank.file_fetch_multiplier,
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
            folder_limit: 3,
            file_limit: 2,
        };
        let rerank = RerankConfig {
            enabled: true,
            code_fetch_multiplier: 4,
            readme_fetch_multiplier: 2,
            crate_fetch_multiplier: 1,
            module_doc_fetch_multiplier: 2,
            folder_fetch_multiplier: 2,
            file_fetch_multiplier: 2,
        };
        let fetched = fetch_limits(&config, &rerank);
        assert_eq!(fetched.code_limit, 20);
        assert_eq!(fetched.readme_limit, 4);
        assert_eq!(fetched.crate_limit, 3);
        assert_eq!(fetched.module_doc_limit, 6);
        assert_eq!(fetched.folder_limit, 6);
        assert_eq!(fetched.file_limit, 4);
    }

    #[test]
    fn test_fetch_limits_disabled() {
        let config = RetrievalConfig {
            code_limit: 5,
            readme_limit: 2,
            crate_limit: 3,
            module_doc_limit: 3,
            folder_limit: 0,
            file_limit: 0,
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
        assert_eq!(fetched.folder_limit, 0);
        assert_eq!(fetched.file_limit, 0);
    }

    #[test]
    fn test_retrieval_config_default_folder_limit_zero() {
        // A2 safety: default ships with folder_limit=0 so A2 ingest doesn't
        // change answers until A3 opens the gate per-intent.
        assert_eq!(RetrievalConfig::default().folder_limit, 0);
    }

    #[test]
    fn test_retrieval_config_default_file_limit_zero() {
        // A4 safety: default ships with file_limit=0; RoutingTable::default
        // supplies per-intent values.
        assert_eq!(RetrievalConfig::default().file_limit, 0);
    }

    #[test]
    fn test_rerank_config_default() {
        let rc = RerankConfig::default();
        assert!(!rc.enabled);
        assert_eq!(rc.code_fetch_multiplier, 4);
        assert_eq!(rc.crate_fetch_multiplier, 1);
        assert_eq!(rc.file_fetch_multiplier, 2);
    }

    #[test]
    fn test_hybrid_config_default() {
        let hc = HybridConfig::default();
        assert!(hc.enabled);
        assert!((hc.rrf_k - 60.0).abs() < f32::EPSILON);
    }
}
