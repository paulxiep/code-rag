# Quality Harness Report

**Label:** post_b1_no_rerank | **Date:** 2026-04-04T08:06:25.955843300+00:00 | **Commit:** eb295c2 | **Model:** BGESmallENV15_384
**Dataset:** data/test_queries.json (43 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.69 |
| recall@10 | 0.69 |
| MRR | 0.68 |
| Intent accuracy | 62% |
| Latency p50 | 58ms |
| Latency p95 | 84ms |
| Recall-scored queries | 26 |
| No-expectation queries | 17 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 4 | 0.75 | 0.75 | 50% |
| implementation | 15 | 0.77 | 0.77 | 73% |
| overview | 8 | 1.00 | 1.00 | 62% |
| relationship | 5 | 0.38 | 0.38 | 40% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-app-state | "How is application state initialized?" | state.rs, AppState | code:state (64%), code:state (64%), code:state (64%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:test_record_and_retrieve (64%), code:to_retrieved_items (63%), code:RetrievedItem (63%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:CodeAnalyzer (63%), code:grammar (63%), code:LanguageHandler (63%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | crate:tui (59%), crate:news (57%), crate:storage (57%) |

## Warnings

- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'state.rs' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
