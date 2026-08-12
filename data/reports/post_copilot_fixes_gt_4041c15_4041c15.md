# Quality Harness Report

**Label:** post_copilot_fixes_gt_4041c15 | **Date:** 2026-08-10T14:30:27.913279400+00:00 | **Commit:** 4041c15 | **Model:** BGESmallENV15_384
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (97 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.59 |
| recall@10 | 0.68 |
| recall@pool | 0.71 |
| MRR | 0.60 |
| Intent accuracy | 100% |
| Latency p50 | 6076ms |
| Latency p95 | 8208ms |
| Recall-scored queries | 89 |
| No-expectation queries | 7 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | recall@pool | Intent Acc |
|--------|---------|----------|-----------|-------------|------------|
| comparison | 13 | 0.62 | 0.73 | 0.75 | 100% |
| implementation | 29 | 0.57 | 0.65 | 0.65 | 100% |
| overview | 33 | 0.67 | 0.73 | 0.81 | 100% |
| relationship | 21 | 0.49 | 0.60 | 0.60 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-llm-generation | "How does the LLM generate responses?" | generator.rs, generate | code:LLMPatcher (99%), code:generate_chart_code (90%), code:complete (78%) |
| impl-app-state | "How is application state initialized?" | AppState | code:initialize (5%), code:state (0%), code:state (0%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:collect_calls_recursive (1%), code:collect_calls_recursive (0%), code:collect_calls_recursive (0%) |
| rel-error-handling | "How do errors propagate through the system?" | error.rs | file:caravan/rpc/rust/caravan-rpc/src/errors.rs (0%), code:qe (0%), code:oc (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:parser (2%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | folder:code-rag/src/store (0%), code:Bell (0%), folder:concurrens/server/modules/object-store (0%) |
| edge-multi-project | "How do code-rag-ingest and code-rag-engine interact?" | code-rag-ingest | module_doc:src (99%), module_doc:src (98%), readme:code-rag/README.md (98%) |
| b4-impl-news-agent | "How does the news agent collect data?" | news | code:collect_triggered (58%), code:update_sim_data (25%), code:triggered_by_news (14%) |
| b4-rel-storage-consumers | "What uses the storage crate?" | crates/server, crates/simulation, server, simulation | module_doc:src (97%), crate:storage (95%), module_doc:src (78%) |
| b5-sig-query | "Functions that return Result<T, Error>" | retriever.rs, retrieve | code:qe (100%), code:F (100%), code:oc (100%) |
| b5-body-query | "Which function parses JSON configs?" | from_json_str | code:Qs (99%), code:parseProviderForm (98%), code:r (97%) |
| a3-api-folder | "What's in the src/api folder?" | code-rag/src/api | folder:concurrens/server/modules/api/src (100%), file:code-rag/crates/code-rag-ui/src/api.rs (96%), module_doc:src (60%) |
| a4-retriever-file | "What does retriever.rs do?" | code-rag/src/engine/retriever.rs | code:FileChunk (89%), file:code-rag/src/harness/matching.rs (62%), file:code-rag/crates/code-rag-core/src/retriever.rs (59%) |
| a4-language-handlers | "Show me files that define language handlers." | code-rag/crates/code-rag-ingest/src/ingestion/languages/rust.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/python.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/typescript.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/go.rs | file:code-rag/crates/code-rag-ingest/src/ingestion/languages/mod.rs (63%), folder:code-rag/crates/code-rag-ingest/src/ingestion/languages (51%), code:handler_for_path (31%) |
| a5-main-components | "What are the main components of code-rag?" | code-rag/README.md | folder:code-rag/crates/code-rag-ui/src/components (100%), folder:code-rag/crates/code-rag-ui/src (100%), file:code-rag/crates/code-rag-ui/src/main.rs (99%) |

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

- Expected file 'code-rag/src/engine/retriever.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/python.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/typescript.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/rust.rs' referenced in test cases but never found in any results
- Expected file 'runner.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/src/api' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/go.rs' referenced in test cases but never found in any results
- Expected file 'generator.rs' referenced in test cases but never found in any results
- Expected file 'error.rs' referenced in test cases but never found in any results
