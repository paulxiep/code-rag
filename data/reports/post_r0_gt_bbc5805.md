# Quality Harness Report

**Label:** post_r0_gt | **Date:** 2026-06-08T14:16:27.916072600+00:00 | **Commit:** bbc5805 | **Model:** BGESmallENV15_384
**Completed tracks:** r0
**Dataset:** data/test_queries.json (90 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.57 |
| recall@10 | 0.67 |
| recall@pool | 0.68 |
| MRR | 0.59 |
| Intent accuracy | 100% |
| Latency p50 | 98ms |
| Latency p95 | 254ms |
| Recall-scored queries | 82 |
| No-expectation queries | 7 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | recall@pool | Intent Acc |
|--------|---------|----------|-----------|-------------|------------|
| comparison | 13 | 0.62 | 0.73 | 0.75 | 100% |
| implementation | 29 | 0.53 | 0.61 | 0.61 | 100% |
| overview | 28 | 0.69 | 0.81 | 0.83 | 100% |
| relationship | 19 | 0.45 | 0.55 | 0.55 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-app-state | "How is application state initialized?" | state.rs, AppState | code:state (64%), code:state (64%), code:state (64%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:get (63%), code:to_retrieved_items (63%), code:RetrievedItem (63%) |
| rel-error-handling | "How do errors propagate through the system?" | error.rs | code:EvaluationResultError (63%), code:HintsResultError (63%), code:error_result (62%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | file:code-rag/crates/code-rag-ingest/src/ingestion/languages/python.rs (63%), code:CodeAnalyzer (63%), code:grammar (63%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | crate:tui (59%), crate:news (57%), crate:storage (57%) |
| b4-adv-and-not-comp | "Tell me about ingestion and processing" | services/processing, processing | code:run (61%), folder:code-rag/crates/code-rag-ingest/src/ingestion (60%), file:cioport/internal/ingest/ingest.go (60%) |
| b4-impl-news-agent | "How does the news agent collect data?" | news | code:iter (75%), code:triggered_by_news (60%), code:AgentData (59%) |
| b4-rel-storage-consumers | "What uses the storage crate?" | crates/server, crates/simulation, server, simulation | crate:storage (84%), crate:news (69%), code:embed_and_store_crates (64%) |
| b5-sig-query | "Functions that return Result<T, Error>" | retriever.rs, retrieve | code:error_result (68%), code:error_result (68%), code:error_result (67%) |
| b5-body-query | "Which function parses JSON configs?" | from_json_str | code:DatabaseConfig (65%), file:invoice-parse/libs/shared-rs/src/config.rs (65%), code:from (64%) |
| a3-api-folder | "What's in the src/api folder?" | code-rag/src/api | folder:concurrens/server/modules/api/src (79%), code:backend_app (75%), file:code-rag/crates/code-rag-engine/src/folder.rs (68%) |
| a4-retriever-file | "What does retriever.rs do?" | code-rag/src/engine/retriever.rs | file:code-rag/crates/code-rag-core/src/retriever.rs (69%), file:code-rag/crates/code-rag-engine/src/retriever.rs (68%), readme:invoice-parse/libs/shared-rs/README.md (64%) |
| a4-language-handlers | "Show me files that define language handlers." | code-rag/crates/code-rag-ingest/src/ingestion/languages/rust.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/python.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/go.rs | folder:code-rag/crates/code-rag-ingest/src/ingestion/languages (71%), file:code-rag/crates/code-rag-ingest/src/ingestion/languages/mod.rs (70%), code:handler_for_path (67%) |

## Warnings

- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/rust.rs' referenced in test cases but never found in any results
- Expected file 'state.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'runner.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/src/engine/retriever.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/python.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/go.rs' referenced in test cases but never found in any results
- Expected file 'error.rs' referenced in test cases but never found in any results
- Expected file 'generator.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/src/api' referenced in test cases but never found in any results
