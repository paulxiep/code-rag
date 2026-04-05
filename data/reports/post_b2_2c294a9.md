# Quality Harness Report

**Label:** post_b2 | **Date:** 2026-04-04T16:51:38.588381100+00:00 | **Commit:** 2c294a9 | **Model:** BGESmallENV15_384
**Completed tracks:** b1, b2
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.61 |
| recall@10 | 0.64 |
| MRR | 0.49 |
| Intent accuracy | 58% |
| Latency p50 | 2641ms |
| Latency p95 | 3569ms |
| Recall-scored queries | 32 |
| No-expectation queries | 17 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 5 | 0.60 | 0.70 | 40% |
| implementation | 18 | 0.72 | 0.72 | 67% |
| overview | 8 | 1.00 | 1.00 | 62% |
| relationship | 7 | 0.33 | 0.42 | 43% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-tree-sitter-parsing | "How does tree-sitter parse code?" | parser.rs, analyze_with_handler | code:extract_calls_from (98%), code:extract_doc (97%), code:extract_calls_from (97%) |
| impl-app-state | "How is application state initialized?" | state.rs, AppState | code:apply_drift (17%), code:_ensure_group (0%), code:get_status (0%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | module_doc:src (3%), code:test_ts_extract_calls_dedup (3%), code:extract_calls (3%) |
| rel-error-handling | "How do errors propagate through the system?" | error.rs | code:test_classifier_build_propagates_error (23%), code:ErrorBoundary (0%), readme:invoice-parse/README.md (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:analyze_with_handler (1%) |
| comp-embed-vs-search | "What is the difference between embedding and vector search?" | embedder.rs | module_doc:src (3%), readme:code-rag/README.md (3%), readme:invoice-parse/README.md (3%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | code:TestLocalFsBlobStore (0%), code:reset (0%), code:test_exists (0%) |
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | code:sample_dataset (4%), code:test_matches_file_exact (0%), code:test_evaluate_hits_excluded_violation (0%) |
| b2-relationship | "How does VectorStore relate to Embedder?" | vector_store.rs | code:embed_and_store_all (3%), code:embed_and_store_module_docs (3%), code:embed_and_store_readme (3%) |

## Min Relevant Failures

| ID | Query | Required | Found |
|----|-------|----------|-------|
| overview-main-components | "What are the main components of this project?" | ? | 0 |
| overview-tech-stack | "What technologies does this project use?" | ? | 0 |
| smoke-retrieval-overview | "Give me an overview of the retrieval system" | ? | 0 |
| smoke-code-structure | "How is the codebase organized?" | ? | 0 |
| smoke-ingestion-quality | "How does the system index source code?" | ? | 0 |
| smoke-search-pipeline | "How does the search pipeline process a query?" | ? | 0 |
| smoke-api-endpoints | "What API endpoints does the server expose?" | ? | 0 |
| smoke-wasm-standalone | "How does the standalone WASM mode work?" | ? | 0 |

## Warnings

- Expected file 'parser.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'error.rs' referenced in test cases but never found in any results
