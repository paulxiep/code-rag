# Quality Harness Report

**Label:** post_a4_fresh | **Date:** 2026-04-18T08:40:31.701064400+00:00 | **Commit:** a02b170 | **Model:** BGESmallENV15_384
**Completed tracks:** a1, a2, a3, a4
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (87 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.72 |
| recall@10 | 0.79 |
| recall@pool | 0.80 |
| MRR | 0.73 |
| Intent accuracy | 70% |
| Latency p50 | 1873ms |
| Latency p95 | 2459ms |
| Recall-scored queries | 79 |
| No-expectation queries | 8 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | recall@pool | Intent Acc |
|--------|---------|----------|-----------|-------------|------------|
| comparison | 12 | 0.69 | 0.77 | 0.79 | 100% |
| implementation | 29 | 0.74 | 0.79 | 0.79 | 62% |
| overview | 26 | 0.82 | 0.89 | 0.90 | 77% |
| relationship | 19 | 0.59 | 0.68 | 0.68 | 53% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), file:code-rag/crates/code-raptor/src/ingestion/languages/python.rs (4%) |
| b4-adv-and-not-comp | "Tell me about ingestion and processing" | services/processing, ingestion, processing | code:QueueAMessage (34%), code:QueueAMessage (4%), code:run_ingestion (3%) |
| b4-impl-news-agent | "How does the news agent collect data?" | news | code:collect_triggered (58%), code:update_sim_data (25%), code:triggered_by_news (14%) |
| b4-rel-storage-consumers | "What uses the storage crate?" | crates/simulation, simulation | module_doc:src (97%), crate:storage (95%), module_doc:src (79%) |
| b5-sig-query | "Functions that return Result<T, Error>" | retriever.rs, retrieve | code:fetchJson (100%), code:AppResult (75%), code:Result (70%) |
| a4-language-handlers | "Show me files that define language handlers." | code-rag/crates/code-raptor/src/ingestion/languages/rust.rs, code-rag/crates/code-raptor/src/ingestion/languages/python.rs, code-rag/crates/code-raptor/src/ingestion/languages/typescript.rs | file:code-rag/crates/code-raptor/src/ingestion/languages/mod.rs (69%), folder:code-rag/crates/code-raptor/src/ingestion/languages (47%), code:handler_for_path (31%) |

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

- Expected file 'code-rag/crates/code-raptor/src/ingestion/languages/rust.rs' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-raptor/src/ingestion/languages/python.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-raptor/src/ingestion/languages/typescript.rs' referenced in test cases but never found in any results
