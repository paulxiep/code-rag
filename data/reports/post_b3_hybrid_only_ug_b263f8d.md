# Quality Harness Report

**Label:** post_b3_hybrid_only_ug | **Date:** 2026-04-05T08:15:34.236639700+00:00 | **Commit:** b263f8d | **Model:** BGESmallENV15_384
**Completed tracks:** b1, b2, b3
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.58 |
| recall@10 | 0.66 |
| MRR | 0.48 |
| Intent accuracy | 58% |
| Latency p50 | 374ms |
| Latency p95 | 603ms |
| Recall-scored queries | 32 |
| No-expectation queries | 17 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 5 | 0.63 | 0.63 | 40% |
| implementation | 18 | 0.61 | 0.69 | 67% |
| overview | 8 | 1.00 | 1.00 | 62% |
| relationship | 7 | 0.42 | 0.58 | 43% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| hero-retriever | "How does the retriever work?" | retriever.rs, retrieve | module_doc:src (2%), code:matches_file (2%), code:to_retrieved_items (2%) |
| impl-tree-sitter-parsing | "How does tree-sitter parse code?" | analyze_with_handler | module_doc:src (3%), module_doc:src (3%), crate:code-rag-types (2%) |
| impl-app-state | "How is application state initialized?" | state.rs, AppState | crate:code-rag-ui (2%), code:state (2%), module_doc:src (2%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:test_record_and_retrieve (3%), code:to_retrieved_items (2%), crate:quant-trading-gym (2%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | readme:code-rag/README.md (3%), module_doc:src (3%), module_doc:src (2%) |
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | code:matches_file (2%), crate:tui (2%), code:get (2%) |

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

- Expected file 'state.rs' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'retriever.rs' referenced in test cases but never found in any results
