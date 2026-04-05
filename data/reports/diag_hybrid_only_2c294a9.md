# Quality Harness Report

**Label:** diag_hybrid_only | **Date:** 2026-04-04T16:52:54.213996+00:00 | **Commit:** 2c294a9 | **Model:** BGESmallENV15_384
**Completed tracks:** b1, b2
**Hybrid search:** BM25 + semantic (RRF fusion)
**Dataset:** data/test_queries.json (49 queries)

## Aggregate

| Metric | Value |
|--------|-------|
| recall@5 | 0.41 |
| recall@10 | 0.58 |
| MRR | 0.31 |
| Intent accuracy | 58% |
| Latency p50 | 64ms |
| Latency p95 | 81ms |
| Recall-scored queries | 32 |
| No-expectation queries | 17 |

## By Intent

| Intent | Queries | recall@5 | recall@10 | Intent Acc |
|--------|---------|----------|-----------|------------|
| comparison | 5 | 0.60 | 0.73 | 40% |
| implementation | 18 | 0.42 | 0.56 | 67% |
| overview | 8 | 1.00 | 1.00 | 62% |
| relationship | 7 | 0.17 | 0.50 | 43% |

## Failures (recall@5 = 0)

| ID | Query | Expected | Got |
|----|-------|----------|-----|
| hero-retriever | "How does the retriever work?" | retriever.rs, retrieve | code:QueryIntent (2%), module_doc:src (2%), readme:invoice-parse/README.md (2%) |
| hero-chat-endpoint | "How does the chat endpoint work?" | handlers.rs, chat | module_doc:src (3%), code:ChatView (3%), code:QueryIntent (2%) |
| impl-incremental-ingestion | "How does incremental ingestion detect changes?" | reconcile | code:run_incremental_ingestion (3%), code:QueryIntent (2%), readme:invoice-parse/README.md (2%) |
| impl-tree-sitter-parsing | "How does tree-sitter parse code?" | analyze_with_handler | module_doc:src (3%), module_doc:src (3%), code:QueryIntent (2%) |
| impl-app-state | "How is application state initialized?" | state.rs, AppState | crate:code-rag-ui (2%), code:_ensure_group (2%), code:state (2%) |
| rel-what-calls-retrieve | "What calls the retrieve function?" | handlers.rs | code:QueryIntent (2%), crate:quant-trading-gym (2%), crate:simulation (2%) |
| rel-embedder-consumers | "What modules use the Embedder?" | state.rs | readme:invoice-parse/README.md (3%), module_doc:src (2%), code:standalone_app (2%) |
| rel-language-implementations | "What languages are supported for parsing?" | languages/mod.rs, handler_for_path | module_doc:src (3%), readme:invoice-parse/README.md (3%), readme:code-rag/README.md (3%) |
| comp-chunk-types | "What is the difference between CodeChunk and ReadmeChunk?" | CodeChunk | readme:code-rag/README.md (3%), module_doc:src (3%), readme:invoice-parse/README.md (3%) |
| edge-ambiguous | "Tell me about the store" | vector_store.rs | readme:code-rag/README.md (3%), module_doc:src (3%), readme:quant-trading-gym/README.md (3%) |
| b2-exact-identifier | "Show me the Retriever" | retriever.rs, retrieve | code:QueryIntent (2%), crate:tui (2%), module_doc:src (2%) |
| b2-relationship | "How does VectorStore relate to Embedder?" | embedder.rs | code:embed_and_store_all (3%), code:QueryIntent (2%), module_doc:src (2%) |

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

- Expected file 'handlers.rs' referenced in test cases but never found in any results
- Expected file 'state.rs' referenced in test cases but never found in any results
- Expected file 'rust.rs' referenced in test cases but never found in any results
- Expected file 'retriever.rs' referenced in test cases but never found in any results
