# Quality Harness Report

**Label:** post_b3_dual_gate_gt | **Date:** 2026-04-05T08:35:09.659486900+00:00 | **Commit:** b263f8d | **Model:** BGESmallENV15_384
**Completed tracks:** b1, b2, b3
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.78 |
| recall@10 | 0.78 |
| MRR | 0.69 |
| Intent accuracy | 100% |
| Latency p50 | 1316ms |
| Latency p95 | 1679ms |
| Recall-scored queries | 24 |
| No-expectation queries | 14 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 4 | 0.67 | 0.67 | 100% |
| implementation | 15 | 0.90 | 0.90 | 100% |
| overview | 8 | 1.00 | 1.00 | 100% |
| relationship | 5 | 0.38 | 0.38 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:collect_calls_recursive (1%), code:RetrievalConfig (0%), code:collect_calls_recursive (0%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:LanguageHandler (91%), readme:invoice-parse/README.md (7%), code:CodeAnalyzer (1%) |

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

## Warnings

- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
