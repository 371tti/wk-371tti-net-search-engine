use std::sync::Arc;

use kurosabi::context::ContextMiddleware;

use super::index::IndexPool;

#[derive(Clone)]
pub struct SearchContext {
    pub index_pool: Arc<IndexPool>,
}

impl ContextMiddleware<SearchContext> for SearchContext {
}
