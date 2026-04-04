# Quality Harness Report

**Label:** diag | **Date:** 2026-04-04T16:56:17.716589400+00:00 | **Commit:** 2c294a9 | **Model:** BGESmallENV15_384
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (6 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.58 |
| recall@10 | 0.67 |
| MRR | 0.50 |
| Intent accuracy | 33% |
| Latency p50 | 75ms |
| Latency p95 | 102ms |
| Recall-scored queries | 6 |
| No-expectation queries | 0 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 1 | 1.00 | 1.00 | 0% |
| implementation | 3 | 0.50 | 0.50 | 33% |
| relationship | 2 | 0.50 | 0.75 | 50% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | code:QueryIntent (2%), crate:tui (2%), module_doc:src (2%) |
| b2-relationship | "How does VectorStore relate to Embedder?" | embedder.rs | code:embed_and_store_all (3%), code:QueryIntent (2%), module_doc:src (2%) |

## Warnings

- Expected file 'retriever.rs' referenced in test cases but never found in any results
