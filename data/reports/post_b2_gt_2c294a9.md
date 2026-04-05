# Quality Harness Report

**Label:** post_b2_gt | **Date:** 2026-04-04T16:57:46.365255+00:00 | **Commit:** 2c294a9 | **Model:** BGESmallENV15_384
**Completed tracks:** b1, b2
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.58 |
| recall@10 | 0.65 |
| MRR | 0.49 |
| Intent accuracy | 100% |
| Latency p50 | 1339ms |
| Latency p95 | 1682ms |
| Recall-scored queries | 24 |
| No-expectation queries | 14 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 4 | 0.12 | 0.38 | 100% |
| implementation | 15 | 0.77 | 0.77 | 100% |
| overview | 8 | 1.00 | 1.00 | 100% |
| relationship | 5 | 0.25 | 0.38 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-tree-sitter-parsing | "How does tree-sitter parse code?" | parser.rs, analyze_with_handler | code:extract_calls_from (98%), code:extract_doc (97%), code:extract_calls_from (97%) |
| impl-app-state | "How is application state initialized?" | state.rs, AppState | code:apply_drift (17%), code:_ensure_group (0%), code:get_status (0%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | module_doc:src (3%), code:test_ts_extract_calls_dedup (3%), code:extract_calls (3%) |
| rel-error-handling | "How do errors propagate through the system?" | error.rs | readme:invoice-parse/README.md (3%), module_doc:src (3%), code:test_invalid_syntax_returns_empty (3%) |
| rel-language-implementations | "What languages are supported for parsing?" | handler_for_path | module_doc:src (3%), readme:invoice-parse/README.md (3%), crate:code-rag-types (2%) |
| comp-chunk-types | "What is the difference between CodeChunk and ReadmeChunk?" | code-rag-types/src/lib.rs, CodeChunk, ReadmeChunk | readme:code-rag/README.md (3%), module_doc:src (3%), readme:quant-trading-gym/README.md (3%) |
| comp-rust-python-handler | "How do the Rust and Python language handlers differ?" | rust.rs, RustHandler | readme:invoice-parse/README.md (3%), readme:code-rag/README.md (3%), code:LanguageHandler (3%) |
| comp-embed-vs-search | "What is the difference between embedding and vector search?" | embedder.rs | module_doc:src (3%), readme:code-rag/README.md (3%), readme:invoice-parse/README.md (3%) |

## Exclusion Violations

| ID | Query | Excluded File | Matched Item |
|----|-------|---------------|--------------|
| b2-conceptual-query | "How does database migration work?" | embedder.rs | (in results) |

## Min Relevant Failures

| ID | Query | Required | Found |
|----|-------|----------|-------|
| overview-main-components | "What are the main components of this project?" | ? | 0 |
| overview-tech-stack | "What technologies does this project use?" | ? | 0 |
| b2-exact-identifier | "Give me an overview of the retrieval system" | ? | 0 |
| b2-relationship | "How does the system index source code?" | ? | 0 |
| b2-comparison | "How does the search pipeline process a query?" | ? | 0 |

## Warnings

- Expected file 'parser.rs' referenced in test cases but never found in any results
