mod tokenize;
mod context;
mod collect;
mod http_client;
mod engine;
mod utils;
mod handler;

use kurosabi::Kurosabi;
use log::info;
use tokio::signal;
use crate::{context::SearchContext, handler::{add::AddHandler, del::DelHandler, meta::MetaHandler, search::SearchHandler}};
use std::sync::atomic::{AtomicBool, Ordering};

pub const INDEX_DIR: &str = "./index_data";
pub const SCRAPER_API_URL: &str = "http://192.168.0.81/scraping?url=";
pub const MAX_DESC_LENGTH: usize = 200; // 説明文の最大長
pub const MAX_TITLE_LENGTH: usize = 100; // タイトルの最大長
pub const MAX_SEARCH_RESULTS: usize = 1000; // 検索結果の最大数
pub const DEFAULT_SEARCH_RESULTS: usize = 20; // 検索結果のデフォルト数

static CTRL_C_SAVED: AtomicBool = AtomicBool::new(false);
pub static BLOCK_INDEX_ACCESS: AtomicBool = AtomicBool::new(false);


#[tokio::main(flavor = "multi_thread", worker_threads = 32)]
async fn main() {
    env_logger::try_init_from_env(env_logger::Env::default().default_filter_or("debug")).unwrap_or_else(|_| ());
    info!("Logger initialized");
    let context = SearchContext::new(INDEX_DIR);

    let context_clone = context.clone();

    // Ctrl+C ハンドラを先にセット
    tokio::spawn(async move {
        if let Err(e) = signal::ctrl_c().await {
            log::error!("Failed to install Ctrl+C handler: {}", e);
            return;
        }
        info!("Ctrl+C detected, saving index and shutting down...");
        info!("Blocking index access during shutdown");
        BLOCK_INDEX_ACCESS.store(true, Ordering::SeqCst);
        info!("Waiting for ongoing write operations to complete");
        context_clone.index_pool.wait_for_writing();
        info!("Ongoing write operations completed");
        if CTRL_C_SAVED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            log::info!("Flushing index to disk...");
            context_clone.index_pool.save(INDEX_DIR).await.unwrap_or_else(|e| {
                log::error!("Index save failed: {}", e);
            });
            log::info!("Shutdown complete.");
        } else {
            log::warn!("Ctrl+C received again; already saving / shutting down.");
        }
        // 明示終了（必要なければ削除）
        std::process::exit(0);
    });

    let mut kurosabi = Kurosabi::with_context(context);

    kurosabi.get("/status", |mut c| async move {
        let count = c.c.index_pool.counter.load(Ordering::SeqCst);
        let result = serde_json::json!({
            "status": "ok",
            "documents": count,
        });
        c.res.json_value(&result);
        c.res.set_status(200);
        c
    });

    kurosabi.get("/ping", |mut c| async move {
        c.res.text("pong");
        c
    });

    kurosabi.post("/meta", |c| async move { MetaHandler::meta(c).await });
    kurosabi.post("/token_freq", |c| async move { MetaHandler::token_freq(c).await });
    kurosabi.post("/add", |c| async move { AddHandler::add(c).await });
    kurosabi.get("/del/*", |c| async move { DelHandler::del(c).await });
    kurosabi.get("/search", |c| async move { SearchHandler::search(c).await });

    kurosabi.not_found_handler(|mut c| async move {
        c.res.text("Not Found");
        c.res.set_status(404);
        c
    });

    kurosabi
        .server()
        .port(90)
        .thread(32)
        .queue_size(3000)
        .host([0,0,0,0])
        .build()
        .run_async()
        .await;
}





