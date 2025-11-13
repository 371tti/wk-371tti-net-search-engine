use kurosabi::kurosabi::Context;
use log::warn;

use crate::{BLOCK_INDEX_ACCESS, collect::IndexRes, context::SearchContext};

pub struct DelHandler;

impl DelHandler {
    // URL削除ハンドラ
    pub async fn del(mut c: Context<SearchContext>) -> Context<SearchContext> {
        if BLOCK_INDEX_ACCESS.load(std::sync::atomic::Ordering::SeqCst) {
            warn!("Index access is currently blocked");
            let result = IndexRes::Failed { error: "Index access is currently blocked".to_string() };
            c.res.json_value(&serde_json::to_value(&result).unwrap());
            c.res.set_status(503);
            return c;
        }
        // パスパラメータからurlを取得
        let full_path = &c.req.path.path;
        let del_part = if let Some(idx) = full_path.find("/del/") {
            &full_path[idx + 5..]
        } else {
            full_path
        };

        let success = c.c.index_pool.del_document(del_part).await;

        if success {
            let result = serde_json::json!({
                "success": true,
                "url": del_part,
            });
            c.res.json_value(&result);
            c.res.set_status(200);
        } else {
            let result = serde_json::json!({
                "success": false,
                "error": "Document not found",
            });
            c.res.json_value(&result);
            c.res.set_status(404);
        }
        c
    }
}