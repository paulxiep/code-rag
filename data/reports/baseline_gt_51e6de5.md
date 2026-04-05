# Quality Harness Report

**Label:** baseline_gt | **Date:** 2026-04-02T17:17:15.653423300+00:00 | **Commit:** 51e6de5 | **Model:** BGESmallENV15_384
**Dataset:** data/test_queries.json (43 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.67 |
| recall@10 | 0.67 |
| MRR | 0.61 |
| Intent accuracy | 100% |
| Latency p50 | 57ms |
| Latency p95 | 80ms |
| Recall-scored queries | 24 |
| No-expectation queries | 8 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 4 | 0.75 | 0.75 | 100% |
| implementation | 15 | 0.70 | 0.70 | 100% |
| overview | 8 | 1.00 | 1.00 | 100% |
| relationship | 5 | 0.38 | 0.38 | 100% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| impl-app-state | "How is application state initialized?" | state.rs, AppState | code:state (64%), code:state (64%), code:state (64%) |
| impl-export | "How does the export subcommand work?" | export.rs, run_export | code:test_jsdoc_with_export (64%), code:Cli (64%), code:handleCommand (62%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:test_record_and_retrieve (64%), code:retrieve (63%), code:RetrievalResult (63%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | code:CodeAnalyzer (63%), code:grammar (63%), code:LanguageHandler (63%) |

## Warnings

- Expected file 'state.rs' referenced in test cases but never found in any results
- Expected file 'languages/mod.rs' referenced in test cases but never found in any results
- Expected file 'export.rs' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
