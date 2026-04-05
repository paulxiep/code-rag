# Quality Harness Report

**Label:** pre_b2_gt | **Date:** 2026-04-04T17:00:48.990678300+00:00 | **Commit:** 2c294a9 | **Model:** BGESmallENV15_384
**Completed tracks:** b1
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.77 |
| recall@10 | 0.79 |
| MRR | 0.64 |
| Intent accuracy | 100% |
| Latency p50 | 1216ms |
| Latency p95 | 1732ms |
| Recall-scored queries | 24 |
| No-expectation queries | 14 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 4 | 0.75 | 0.88 | 100% |
| implementation | 15 | 0.87 | 0.87 | 100% |
| overview | 8 | 1.00 | 1.00 | 100% |
| relationship | 5 | 0.38 | 0.38 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-tree-sitter-parsing | "How does tree-sitter parse code?" | parser.rs, analyze_with_handler | code:extract_calls_from (98%), code:extract_calls_from (97%), code:extract_calls_from (97%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:test_record_and_retrieve (64%), code:get (63%), code:get (61%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:CodeAnalyzer (63%), code:grammar (63%), code:LanguageHandler (63%) |

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

- Expected file 'parser.rs' referenced in test cases but never found in any results
