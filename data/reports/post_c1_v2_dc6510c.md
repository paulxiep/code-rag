# Quality Harness Report

**Label:** post_c1_v2 | **Date:** 2026-04-08T18:00:53.280798600+00:00 | **Commit:** dc6510c | **Model:** BGESmallENV15_384
**Completed tracks:** c1
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (81 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.67 |
| recall@10 | 0.71 |
| MRR | 0.61 |
| Intent accuracy | 69% |
| Latency p50 | 1418ms |
| Latency p95 | 1820ms |
| Recall-scored queries | 73 |
| No-expectation queries | 8 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 12 | 0.60 | 0.66 | 100% |
| implementation | 27 | 0.72 | 0.75 | 63% |
| overview | 23 | 0.79 | 0.80 | 78% |
| relationship | 18 | 0.50 | 0.57 | 44% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:test_extract_target_term_what_calls (100%), code:retrieve (1%), code:to_retrieved_items (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:CodeAnalyzer (1%) |
| comp-retriever-generator | "Compare the retriever and generator" | retriever.rs, generator.rs | code:test_pre_classify_non_comparison_returns_none (64%), code:from (62%), code:test_record_and_retrieve (61%) |
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | code:test_detect_direction_both (0%), code:test_extract_target_term_what_calls (0%), code:test_record_and_retrieve (0%) |
| b4-ov-dashboard-svc | "What is the dashboard service?" | invoice-parse/services/dashboard, dashboard | code:DashboardData (9%), code:DashboardTabProps (7%), code:useDashboardData (2%) |
| b4-ov-shared-py | "What is shared-py?" | libs/shared-py, shared-py | code:LineItem (96%), crate:shared-rs (2%), crate:output (1%) |
| b4-ov-processing-svc | "What does the processing service do?" | services/processing, processing | module_doc:src (0%), code:QueueAMessage (0%), code:QueueBMessage (0%) |
| b4-comp-retriever-api | "Compare retriever.rs vs standalone_api.rs" | retriever.rs, standalone_api.rs | code:matches_file (66%), code:api_base_url (64%), code:test_to_retrieved_items_ranking (64%) |
| b5-sig-query | "Functions that return Result<T, Error>" | retriever.rs, metrics.rs, retrieve, recall_at_k | code:fetchJson (100%), code:AppResult (75%), code:Result (70%) |
| b5-body-query | "Which function parses JSON configs?" | dataset.rs, load | code:from_json_str (53%), code:from_json_str (45%), code:fetchJson (33%) |

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

- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'invoice-parse/services/dashboard' referenced in test cases but never found in any results
- Expected file 'qtg.py' referenced in test cases but never found in any results
- Expected file 'dataset.rs' referenced in test cases but never found in any results
- Expected file 'metrics.rs' referenced in test cases but never found in any results
- Expected file 'runner.rs' referenced in test cases but never found in any results
- Expected file 'standalone_api.rs' referenced in test cases but never found in any results
- Expected file 'libs/shared-py' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
