use std::sync::Arc;

use kurosabi::context::ContextMiddleware;

use crate::index::IndexPool;

#[derive(Clone)]
pub struct SearchContext {
    pub index_pool: Arc<IndexPool>,
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
        Self { index_pool }
    }
}

impl ContextMiddleware<SearchContext> for SearchContext {
}
