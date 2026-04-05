# Quality Harness Report

**Label:** post_b4_final_gt_v2 | **Date:** 2026-04-05T14:20:46.558234400+00:00 | **Commit:** f492b06 | **Model:** BGESmallENV15_384
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (97 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.71 |
| recall@10 | 0.71 |
| MRR | 0.64 |
| Intent accuracy | 100% |
| Latency p50 | 1476ms |
| Latency p95 | 2981ms |
| Recall-scored queries | 32 |
| No-expectation queries | 54 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 17 | 0.61 | 0.61 | 100% |
| implementation | 30 | 0.83 | 0.83 | 100% |
| overview | 20 | 0.50 | 0.50 | 100% |
| relationship | 19 | 0.50 | 0.50 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:collect_calls_recursive (1%), code:RetrievalConfig (0%), code:collect_calls_recursive (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:CodeAnalyzer (1%) |
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | code:matches_file (1%), code:brute_force_search (0%), code:to_retrieved_items (0%) |
| b4-ov-qtg-py | "What is qtg.py?" | qtg.py | code:LineItem (0%), readme:quant-trading-gym/README.md (0%), readme:quant-trading-gym/frontend/README.md (0%) |
| b4-comp-retriever-api | "Compare retriever.rs vs standalone_api.rs" | retriever.rs, standalone_api.rs | code:matches_file (66%), code:test_evaluate_hits_partial (63%), code:QueryIntent (63%) |

## Min Relevant Failures

| ID | Query | Required | Found |
|----|-------|----------|-------|
| overview-main-components | "What are the main components of this project?" | ? | 0 |
| overview-tech-stack | "What technologies does this project use?" | ? | 0 |

## Warnings

- Expected file 'standalone_api.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'qtg.py' referenced in test cases but never found in any results
