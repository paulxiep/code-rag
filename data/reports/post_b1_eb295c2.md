# Quality Harness Report

**Label:** post_b1 | **Date:** 2026-04-04T08:04:55.328062600+00:00 | **Commit:** eb295c2 | **Model:** BGESmallENV15_384
**Completed tracks:** b1
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Dataset:** data/test_queries.json (43 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.70 |
| recall@10 | 0.72 |
| MRR | 0.68 |
| Intent accuracy | 62% |
| Latency p50 | 2934ms |
| Latency p95 | 3557ms |
| Recall-scored queries | 26 |
| No-expectation queries | 17 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 4 | 0.69 | 0.69 | 50% |
| implementation | 15 | 0.87 | 0.87 | 73% |
| overview | 8 | 1.00 | 1.00 | 62% |
| relationship | 5 | 0.12 | 0.25 | 40% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-tree-sitter-parsing | "How does tree-sitter parse code?" | parser.rs, analyze_with_handler | code:extract_calls_from (98%), code:extract_calls_from (97%), code:extract_calls_from (97%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:retrieve (9%), code:RetrievedItem (0%), code:to_retrieved_items (0%) |
| rel-error-handling | "How do errors propagate through the system?" | error.rs | code:test_classifier_build_propagates_error (23%), code:ErrorBoundary (0%), readme:invoice-parse/README.md (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:analyze_with_handler (1%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | code:TestLocalFsBlobStore (0%), code:test_exists (0%), module_doc:src (0%) |

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
- Expected file 'error.rs' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
