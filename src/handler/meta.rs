use kurosabi::kurosabi::Context;
use log::warn;

use crate::{BLOCK_INDEX_ACCESS, collect::{IndexRes, MetaReq, MetaRes}, context::SearchContext};

pub struct MetaHandler;

impl MetaHandler {
    // URLメタ情報取得ハンドラ
    pub async fn meta(mut c: Context<SearchContext>) -> Context<SearchContext> {
        if BLOCK_INDEX_ACCESS.load(std::sync::atomic::Ordering::SeqCst) {
            warn!("Index access is currently blocked");
            let result = IndexRes::Failed { error: "Index access is currently blocked".to_string() };
            c.res.json_value(&serde_json::to_value(&result).unwrap());
            c.res.set_status(503);
            return c;
        }
        let meta_req = match c.req.body_de_struct::<MetaReq>().await {
            Ok(v) => v,
            Err(_) => {
                warn!("Missing or invalid request body");
                let result = MetaRes::Failed { error: "Invalid request body".to_string() };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(400);
                return c;
            },
        };

        let url = meta_req.url;
        let meta = match c.c.index_pool.get_meta(&url).await {
            Some(m) => m,
            None => {
                let result = MetaRes::Failed { error: "Document not found".to_string() };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(404);
                return c;
            }
        };

        let result = MetaRes::Success {
            meta,
        };
        c.res.json_value(&serde_json::to_value(&result).unwrap());
        c.res.set_status(200);
        c
    }
}