# Quality Harness Report

**Label:** pseudo_a3_fresh | **Date:** 2026-04-18T08:44:50.317415700+00:00 | **Commit:** a02b170 | **Model:** BGESmallENV15_384
**Completed tracks:** a1, a2, a3
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (87 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.72 |
| recall@10 | 0.77 |
| recall@pool | 0.77 |
| MRR | 0.68 |
| Intent accuracy | 70% |
| Latency p50 | 1804ms |
| Latency p95 | 2313ms |
| Recall-scored queries | 79 |
| No-expectation queries | 8 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | recall@pool | Intent Acc |
|--------|---------|----------|-----------|-------------|------------|
| comparison | 12 | 0.62 | 0.65 | 0.67 | 100% |
| implementation | 29 | 0.74 | 0.79 | 0.79 | 62% |
| overview | 26 | 0.83 | 0.88 | 0.88 | 77% |
| relationship | 19 | 0.63 | 0.68 | 0.68 | 53% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:CodeAnalyzer (1%) |
| comp-retriever-generator | "Compare the retriever and generator" | retriever.rs, generator.rs | code:extract_comparators (63%), code:from (61%), code:from (61%) |
| b4-ov-shared-py | "What is shared-py?" | libs/shared-py, shared-py | code:LineItem (96%), crate:shared-rs (2%), crate:output (1%) |
| b4-impl-news-agent | "How does the news agent collect data?" | news | code:collect_triggered (58%), code:update_sim_data (25%), code:triggered_by_news (14%) |
| b4-comp-retriever-api | "Compare retriever.rs vs standalone_api.rs" | retriever.rs, standalone_api.rs | code:matches_file (67%), code:api_base_url (64%), code:standalone_app (63%) |
| b5-sig-query | "Functions that return Result<T, Error>" | retriever.rs, retrieve | code:fetchJson (100%), code:AppResult (75%), code:Result (70%) |
| a4-language-handlers | "Show me files that define language handlers." | code-rag/crates/code-raptor/src/ingestion/languages/rust.rs, code-rag/crates/code-raptor/src/ingestion/languages/python.rs, code-rag/crates/code-raptor/src/ingestion/languages/typescript.rs | folder:code-rag/crates/code-raptor/src/ingestion/languages (47%), code:handler_for_path (31%), code:LanguageHandler (16%) |

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

- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-raptor/src/ingestion/languages/rust.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-raptor/src/ingestion/languages/typescript.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-raptor/src/ingestion/languages/python.rs' referenced in test cases but never found in any results
