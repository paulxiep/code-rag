# Quality Harness Report

**Label:** post_c1_graphidx | **Date:** 2026-04-09T04:01:58.554437700+00:00 | **Commit:** dc6510c | **Model:** BGESmallENV15_384
**Completed tracks:** c1
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (81 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.67 |
| recall@10 | 0.71 |
| MRR | 0.66 |
| Intent accuracy | 69% |
| Latency p50 | 1595ms |
| Latency p95 | 2183ms |
| Recall-scored queries | 73 |
| No-expectation queries | 8 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 12 | 0.60 | 0.66 | 100% |
| implementation | 27 | 0.72 | 0.77 | 63% |
| overview | 23 | 0.77 | 0.80 | 78% |
| relationship | 18 | 0.51 | 0.57 | 44% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:retrieve (1%), code:to_retrieved_items (0%), code:RetrievedItem (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:CodeAnalyzer (1%) |
| comp-retriever-generator | "Compare the retriever and generator" | retriever.rs, generator.rs | code:from (61%), code:name (61%), code:from (60%) |
| b4-ov-dashboard-svc | "What is the dashboard service?" | invoice-parse/services/dashboard, dashboard | code:DashboardData (9%), code:DashboardTabProps (7%), code:useDashboardData (2%) |
| b4-ov-shared-py | "What is shared-py?" | libs/shared-py, shared-py | code:LineItem (96%), crate:shared-rs (2%), crate:output (1%) |
| b4-ov-processing-svc | "What does the processing service do?" | services/processing, processing | module_doc:src (0%), code:QueueAMessage (0%), code:QueueBMessage (0%) |
| b4-adv-and-not-comp | "Tell me about ingestion and processing" | services/processing, processing | code:QueueAMessage (34%), code:QueueAMessage (4%), code:run_ingestion (0%) |
| b4-impl-news-agent | "How does the news agent collect data?" | news | code:collect_triggered (58%), code:update_sim_data (25%), code:triggered_by_news (14%) |
| b4-comp-retriever-api | "Compare retriever.rs vs standalone_api.rs" | retriever.rs, standalone_api.rs | code:matches_file (66%), code:api_base_url (64%), code:standalone_app (63%) |
| b5-sig-query | "Functions that return Result<T, Error>" | retriever.rs, metrics.rs, retrieve, recall_at_k | code:fetchJson (100%), code:AppResult (75%), code:Result (70%) |
| b5-body-query | "Which function parses JSON configs?" | dataset.rs, load | code:submitConfig (93%), code:from_json_str (53%), code:from_json_str (45%) |

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

- Expected file 'qtg.py' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'invoice-parse/services/dashboard' referenced in test cases but never found in any results
- Expected file 'libs/shared-py' referenced in test cases but never found in any results
- Expected file 'runner.rs' referenced in test cases but never found in any results
- Expected file 'metrics.rs' referenced in test cases but never found in any results
- Expected file 'dataset.rs' referenced in test cases but never found in any results
