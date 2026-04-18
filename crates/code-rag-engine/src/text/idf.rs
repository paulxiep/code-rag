use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Document frequency table for BM25 scoring.
///
/// Built over a corpus at ingestion time, serialized into index.json for the
/// browser, deserialized server-side from LanceDB. Same struct everywhere —
/// no separate "server IdfTable" vs "browser IdfTable".
///
/// Serialized shape is `{num_docs, doc_frequencies}` — matches existing
/// `index.json` produced by pre-A1 exporters, so old bundles still deserialize.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct IdfTable {
    pub num_docs: usize,
    pub doc_frequencies: HashMap<String, usize>,
}

impl IdfTable {
    /// Build an IdfTable from an iterator of documents.
    ///
    /// Each document is tokenized with the canonical `tokenize()` function
    /// before counting. Only unique tokens per document contribute to
    /// `doc_frequencies` (standard IDF construction).
    pub fn build<I, S>(docs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut num_docs = 0usize;
        let mut doc_frequencies: HashMap<String, usize> = HashMap::new();

        for doc in docs {
            num_docs += 1;
            let tokens = super::tokenize::tokenize(doc.as_ref());
            let unique: HashSet<String> = tokens.into_iter().collect();
            for term in unique {
                *doc_frequencies.entry(term).or_insert(0) += 1;
            }
        }

        Self {
            num_docs,
            doc_frequencies,
        }
    }

    /// Standard BM25 IDF: ln( (N - df + 0.5) / (df + 0.5) + 1 ).
    pub fn idf(&self, term: &str) -> f32 {
        let df = *self.doc_frequencies.get(term).unwrap_or(&0) as f32;
        let n = self.num_docs as f32;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }
}
