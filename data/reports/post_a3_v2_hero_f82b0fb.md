# Quality Harness Report

**Label:** post_a3_v2_hero | **Date:** 2026-04-17T14:53:58.163759+00:00 | **Commit:** f82b0fb | **Model:** BGESmallENV15_384
**Completed tracks:** a1, a2, a3
**Reranking:** ms-marco-MiniLM-L-6-v2 (code 4x)
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (3 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 1.00 |
| recall@10 | 1.00 |
| MRR | 0.61 |
| Intent accuracy | 67% |
| Latency p50 | 2164ms |
| Latency p95 | 2191ms |
| Recall-scored queries | 3 |
| No-expectation queries | 0 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| implementation | 1 | 1.00 | 1.00 | 0% |
| overview | 2 | 1.00 | 1.00 | 100% |

