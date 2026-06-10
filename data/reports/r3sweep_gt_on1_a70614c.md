# Quality Harness Report

**Label:** r3sweep_gt_on1 | **Date:** 2026-06-10T04:40:34.047698900+00:00 | **Commit:** a70614c | **Model:** BGESmallENV15_384
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (97 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.58 |
| recall@10 | 0.67 |
| recall@pool | 0.71 |
| MRR | 0.59 |
| Intent accuracy | 100% |
| Latency p50 | 2595ms |
| Latency p95 | 4819ms |
| Recall-scored queries | 89 |
| No-expectation queries | 7 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | recall@pool | Intent Acc |
|--------|---------|----------|-----------|-------------|------------|
| comparison | 13 | 0.62 | 0.73 | 0.75 | 100% |
| implementation | 29 | 0.57 | 0.61 | 0.62 | 100% |
| overview | 33 | 0.63 | 0.77 | 0.84 | 100% |
| relationship | 21 | 0.49 | 0.59 | 0.59 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| hero-chat-endpoint | "How does the chat endpoint work?" | handlers.rs, chat | code:send_chat (0%), code:TrainingExample (0%), code:PromptMessages (0%) |
| impl-llm-generation | "How does the LLM generate responses?" | generator.rs, generate | code:LLMPatcher (99%), code:generate_chart_code (90%), code:complete (78%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | cluster:code-rag/crates/code-rag-engine/src (18%), code:collect_calls_recursive (1%), code:retrieve (0%) |
| rel-error-handling | "How do errors propagate through the system?" | error.rs | code:gemini_retry_on_transient (17%), file:caravan/rpc/rust/caravan-rpc/src/errors.rs (0%), module_doc:src (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | handler_for_path | code:LanguageHandler (91%), cluster:code-rag/crates/code-rag-ingest/src/ingestion (13%), readme:invoice-parse/README.md (7%) |
| b4-impl-news-agent | "How does the news agent collect data?" | news | code:collect_triggered (58%), code:update_sim_data (25%), code:triggered_by_news (14%) |
| b4-rel-storage-consumers | "What uses the storage crate?" | crates/server, crates/simulation, server, simulation | module_doc:src (97%), crate:storage (95%), module_doc:src (78%) |
| b5-sig-query | "Functions that return Result<T, Error>" | retriever.rs, retrieve | code:fetchJson (100%), code:AppResult (75%), code:Result (70%) |
| b5-body-query | "Which function parses JSON configs?" | from_json_str | code:submitConfig (93%), code:_parse_candidate_json (88%), code:parse_json_from_response (82%) |
| a3-api-folder | "What's in the src/api folder?" | code-rag/src/api | folder:concurrens/server/modules/api/src (100%), file:code-rag/crates/code-rag-ui/src/api.rs (96%), module_doc:src (60%) |
| a4-retriever-file | "What does retriever.rs do?" | code-rag/src/engine/retriever.rs | code:FileChunk (89%), file:code-rag/src/harness/matching.rs (62%), file:code-rag/crates/code-rag-core/src/retriever.rs (59%) |
| a4-language-handlers | "Show me files that define language handlers." | code-rag/crates/code-rag-ingest/src/ingestion/languages/rust.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/python.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/typescript.rs, code-rag/crates/code-rag-ingest/src/ingestion/languages/go.rs | cluster:code-rag/crates/code-rag-ingest/src/ingestion (90%), file:code-rag/crates/code-rag-ingest/src/ingestion/languages/mod.rs (63%), folder:code-rag/crates/code-rag-ingest/src/ingestion/languages (51%) |
| a5-main-components | "What are the main components of code-rag?" | code-rag/README.md | folder:code-rag/crates/code-rag-ui/src/components (100%), folder:code-rag/crates/code-rag-ui/src (100%), file:code-rag/crates/code-rag-ui/src/main.rs (99%) |
| r3-main-subsystems | "What are the main subsystems of this codebase and how is it organized?" | crates/code-rag-engine, crates/code-rag-store | folder:quant-trading-gym/crates/simulation/src/subsystems (0%), readme:code-rag/README.md (0%), file:code-rag/src/main.rs (0%) |
| r3-ingestion-subsystem | "What makes up the ingestion and parsing pipeline?" | parser.rs | readme:daccord/data/README.md (24%), folder:invoice-parse/services/ingestion (4%), folder:daccord/src/daccord/ingest (2%) |

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

- Expected file 'generator.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/typescript.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/src/engine/retriever.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/go.rs' referenced in test cases but never found in any results
- Expected file 'error.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/python.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/src/api' referenced in test cases but never found in any results
- Expected file 'runner.rs' referenced in test cases but never found in any results
- Expected file 'handlers.rs' referenced in test cases but never found in any results
- Expected file 'code-rag/crates/code-rag-ingest/src/ingestion/languages/rust.rs' referenced in test cases but never found in any results
