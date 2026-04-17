use super::idf::IdfTable;

/// BM25 parameters. Defaults preserve the browser's pre-A1 behavior (k1=1.2, b=0.75).
#[derive(Clone, Copy, Debug)]
pub struct Bm25Params {
    pub k1: f32,
    pub b: f32,
}

impl Default for Bm25Params {
    fn default() -> Self {
        Self { k1: 1.2, b: 0.75 }
    }
}

/// Score a single (query_tokens, doc_tokens) pair against an IdfTable.
///
/// `avg_doc_len` is passed in explicitly so callers can decide whether to
/// compute it over the full corpus (matches LanceDB semantics) or over the
/// subset being scored (matches pre-A1 browser behavior).
pub fn score(
    query_tokens: &[String],
    doc_tokens: &[String],
    avg_doc_len: f32,
    idf: &IdfTable,
    params: Bm25Params,
) -> f32 {
    if query_tokens.is_empty() || doc_tokens.is_empty() {
        return 0.0;
    }

    let doc_len = doc_tokens.len() as f32;
    let avg = if avg_doc_len > 0.0 {
        avg_doc_len
    } else {
        doc_len.max(1.0)
    };

    let mut total = 0.0_f32;
    for q in query_tokens {
        let tf = doc_tokens.iter().filter(|t| *t == q).count() as f32;
        if tf == 0.0 {
            continue;
        }
        let idf_q = idf.idf(q);
        let num = tf * (params.k1 + 1.0);
        let den = tf + params.k1 * (1.0 - params.b + params.b * doc_len / avg);
        total += idf_q * num / den;
    }
    total
}
