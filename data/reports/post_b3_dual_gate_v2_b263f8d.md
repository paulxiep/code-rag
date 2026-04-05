# Quality Harness Report

**Label:** post_b3_dual_gate_v2 | **Date:** 2026-04-05T08:32:33.780892100+00:00 | **Commit:** b263f8d | **Model:** BGESmallENV15_384
**Completed tracks:** b1, b2, b3
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.75 |
| recall@10 | 0.75 |
| MRR | 0.66 |
| Intent accuracy | 58% |
| Latency p50 | 1384ms |
| Latency p95 | 2191ms |
| Recall-scored queries | 32 |
| No-expectation queries | 17 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 5 | 0.58 | 0.58 | 40% |
| implementation | 18 | 0.83 | 0.83 | 67% |
| overview | 8 | 1.00 | 1.00 | 62% |
| relationship | 7 | 0.50 | 0.50 | 43% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:collect_calls_recursive (1%), code:RetrievalConfig (0%), code:collect_calls_recursive (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:CodeAnalyzer (1%) |
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | code:matches_file (1%), code:brute_force_search (0%), code:to_retrieved_items (0%) |

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
