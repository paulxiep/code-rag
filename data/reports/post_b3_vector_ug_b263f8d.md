# Quality Harness Report

**Label:** post_b3_vector_ug | **Date:** 2026-04-05T08:15:19.974772800+00:00 | **Commit:** b263f8d | **Model:** BGESmallENV15_384
**Completed tracks:** b1, b2, b3
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.66 |
| recall@10 | 0.66 |
| MRR | 0.64 |
| Intent accuracy | 58% |
| Latency p50 | 117ms |
| Latency p95 | 450ms |
| Recall-scored queries | 32 |
| No-expectation queries | 17 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 5 | 0.73 | 0.73 | 40% |
| implementation | 18 | 0.69 | 0.69 | 67% |
| overview | 8 | 1.00 | 1.00 | 62% |
| relationship | 7 | 0.50 | 0.50 | 43% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| hero-retriever | "How does the retriever work?" | retriever.rs, retrieve | code:to_retrieved_items (60%), code:RetrievedItem (59%), code:from (59%) |
| impl-app-state | "How is application state initialized?" | state.rs, AppState | code:state (63%), code:state (63%), code:state (63%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:to_retrieved_items (64%), code:test_record_and_retrieve (63%), code:renderResults (62%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:analyze_with_handler (64%), code:PythonHandler (64%), code:CodeAnalyzer (63%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | crate:tui (59%), crate:news (57%), crate:storage (57%) |
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | crate:tui (63%), code:get (59%), code:ApiError (58%) |

## Warnings

- Expected file 'retriever.rs' referenced in test cases but never found in any results
- Expected file 'state.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Query 'b2-relationship' latency 611ms > 5x median 117ms — possible cold-start or resource issue
