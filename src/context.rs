use std::sync::Arc;

use kurosabi::context::ContextMiddleware;

use crate::{engine::IndexPool, tokenize::SudachiTokenizer};

#[derive(Clone)]
pub struct SearchContext {
    pub index_pool: Arc<IndexPool>,
    pub sudachi_tokenizer: Arc<SudachiTokenizer>,
}

impl SearchContext {
    pub fn new(index_dir: &str) -> Self {
        let index_pool = match IndexPool::load_or_new(index_dir) {
            Ok(pool) => {
                log::info!("Index pool loaded successfully");
                Arc::new(pool)
            },
            Err(e) => {
                panic!("Failed to load or create index pool: {}", e);
            }
        };
        Self { index_pool, sudachi_tokenizer: Arc::new(SudachiTokenizer::new().expect("Config file not found")) }
    }
}

impl ContextMiddleware<SearchContext> for SearchContext {
}
