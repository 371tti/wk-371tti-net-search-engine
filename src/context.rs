use std::sync::{Arc, RwLock};

use dashmap::DashMap;
use kurosabi::context::{ContextMiddleware, DefaultContext};
use tf_idf_vectorizer::{Corpus, TFIDFVectorizer};

#[derive(Clone)]
pub struct SearchContext {
    pub corpus: Arc<Corpus>,
    pub vectorizer: Arc<RwLock<TFIDFVectorizer<u16>>>,
    pub meta: Arc<DashMap<String, SiteMeta>>, // URL -> SiteMeta
}

pub struct SiteMeta {
    pub title: String,
    pub description: String,
    pub favicon: Option<String>,
}

impl SearchContext {
    pub fn new() -> Self {
        let arc_corpus = Arc::new(Corpus::new());
        let arc_vectorizer = Arc::new(
            RwLock::new(
                TFIDFVectorizer::<u16>::new(arc_corpus.clone())
            )
        );
        Self {
            corpus: arc_corpus,
            vectorizer: arc_vectorizer,
            meta: Arc::new(DashMap::new()),
        }
    }

    /// 文字列を先頭100文字に丸める（UTF-8セーフ）
    pub fn trim100(s: &str) -> String {
        s.chars().take(100).collect()
    }

    // 他に必要なメソッドがあればここに追加
}

impl ContextMiddleware<SearchContext> for SearchContext {
}