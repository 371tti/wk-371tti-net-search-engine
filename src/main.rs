mod tokenize;
mod context;
mod collect;
mod http_client;
mod index;


use kurosabi::Kurosabi;
use log::{debug, info, warn, LevelFilter};
use tokio::signal;
use std::{io::Write, sync::atomic::{AtomicBool, Ordering}};
use percent_encoding::percent_decode_str;
use tf_idf_vectorizer::{SimilarityAlgorithm, TokenFrequency};

use crate::{collect::{IndexReq, IndexRes, ScraperResult, SearchRes}, context::SearchContext, http_client::fetch_scraper_api, index::{IndexMeta, Tags}, tokenize::{sudachi_tokenize_large, SudachiMode}};

pub const INDEX_DIR: &str = "./index_data";
pub const SCRAPER_API_URL: &str = "http://localhost:88/url/";
pub const MAX_DESC_LENGTH: usize = 100; // 説明文の最大長
pub const MAX_TITLE_LENGTH: usize = 100; // タイトルの最大長
pub const MAX_SEARCH_RESULTS: usize = 1000; // 検索結果の最大数
pub const DEFAULT_SEARCH_RESULTS: usize = 20; // 検索結果のデフォルト数

static CTRL_C_SAVED: AtomicBool = AtomicBool::new(false);


#[tokio::main]
async fn main() {
    init_logging();
    info!("Logger initialized");
    let context = SearchContext::new(INDEX_DIR);

    let context_clone = context.clone();

    // Ctrl+C ハンドラを先にセット
    tokio::spawn(async move {
        if let Err(e) = signal::ctrl_c().await {
            log::error!("Failed to install Ctrl+C handler: {}", e);
            return;
        }
        if CTRL_C_SAVED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            log::info!("Ctrl+C detected. Flushing index to disk...");
            context_clone.index_pool.save(INDEX_DIR).unwrap_or_else(|e| {
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

    kurosabi.post("/add", |mut c| async move {
        let index_req = match c.req.body_de_struct::<IndexReq>().await {
            Ok(v) => v,
            Err(_) => {
                warn!("Missing or invalid request body");
                let result = IndexRes::Failed { error: "Invalid request body".to_string() };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(400);
                return c;
            },
        };

        let scraper_result = match fetch_scraper_api(&format!("{}{}", SCRAPER_API_URL, index_req.url)).await {
            Ok(res) => res,
            Err(e) => {
                warn!("Failed to fetch scraper API: {}", e);
                let result = IndexRes::Failed { error: format!("Failed to fetch scraper API: {}", e) };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(500);
                return c;
            }
        };

        match scraper_result {
            ScraperResult::Success { results, status: _, url, success: _ } => {
                let body = match results.descriptions.first() {
                    Some(d) => d,
                    None => {
                        warn!("No body text found");
                        let result = IndexRes::Failed { error: "No body text found".to_string() };
                        c.res.json_value(&serde_json::to_value(&result).unwrap());
                        c.res.set_status(404);
                        return c;
                    }
                };

                let title = match index_req.title.or_else(|| results.title.first().cloned()) {
                    Some(t) => t,
                    None => "No Title".to_string(),
                }.chars().take(MAX_TITLE_LENGTH).collect();

                let description = match index_req.descriptions.clone() {
                    Some(d) => d.chars().take(MAX_DESC_LENGTH).collect(),
                    None => body.chars().take(MAX_DESC_LENGTH).collect(), // 先頭500文字を説明に
                };
                
                let favicon: Option<Box<str>> = index_req.favicon.or_else(|| results.favicon.first().cloned()).map(|s| s.into_boxed_str());

                let url = url.into_boxed_str();

                let tags = Tags::from_strs(&index_req.tags);

                let meta = IndexMeta { 
                    id: 0, 
                    url, 
                    title, 
                    description, 
                    favicon, 
                    time: chrono::Utc::now(), 
                    points: 0.0, 
                    tags 
                };

                let tokens = match sudachi_tokenize_large(body, SudachiMode::A, 2000) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("sudachi_tokenize_large error: {}", e);
                        let result = IndexRes::Failed { error: format!("Tokenization error: {}", e) };
                        c.res.json_value(&serde_json::to_value(&result).unwrap());
                        c.res.set_status(500);
                        return c;
                    }
                };

                let token_fq = TokenFrequency::from(&tokens[..]);

                c.c.index_pool.add_document(&token_fq, meta.clone());
                info!("Added URL: {}", meta.url);
                let result = IndexRes::Success { 
                    url: meta.url, 
                    title: meta.title, 
                    favicon: meta.favicon, 
                    tags: meta.tags.tags(), 
                    descriptions: meta.description, 
                };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(200);
                return c;
            }
            ScraperResult::Failed { error , success } => {
                warn!("Scraper API returned error: {}", error);
                let result = IndexRes::Failed { error: format!("Scraper API error: {}", error) };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(500);
                return c;
            }
        }
    });

    kurosabi.get("/search", |mut c| async move {
        // query（URLエンコードされている可能性があるためデコード）
        let query_str = match c.req.path.get_query("query") {
            Some(q) => {
                let decoded = percent_decode_str(&q)
                    .decode_utf8()
                    .map(|cow| cow.into_owned())
                    .unwrap_or(q);
                let trimmed = decoded.trim().to_string();
                if trimmed.is_empty() {
                    let result = SearchRes::Failed { error: "Missing query".to_string() };
                    c.res.json_value(&serde_json::to_value(&result).unwrap());
                    c.res.set_status(400);
                    return c;
                }
                trimmed
            }
            None => {
                let result = SearchRes::Failed { error: "Missing query".to_string() };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(400);
                return c;
            }
        };
        // range パラメータ正規化
        let (range_start, range_end) = parse_range_param(c.req.path.get_query("range"));
        let range = range_start..range_end;
        // algo (URLエンコードの可能性があるためデコードしてから簡易パース)
        let algo_str_raw = c
            .req
            .path
            .get_query("algo")
            .unwrap_or_else(|| "BM25(1.2,0.75)".to_string());
        let algo_str = percent_decode_str(&algo_str_raw)
            .decode_utf8()
            .map(|cow| cow.into_owned())
            .unwrap_or(algo_str_raw);
        let algo = parse_algo(&algo_str);
        // tag (URLエンコードの可能性があるためデコードしてからパース)
        // tag=tag1,tag2,...
        let tag_str = c.req.path.get_query("tag").unwrap_or_default();
        let tag_decoded = percent_decode_str(&tag_str)
            .decode_utf8()
            .map(|cow| cow.into_owned())
            .unwrap_or(tag_str.to_string());
        let tags = Tags::from_strs(&tag_decoded.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>());
        let tag_exclusive = c.req.path.get_query("tag_exclusive")
            .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "true" || v == "1"
            })
            .unwrap_or(false);

        debug!("tag_exclusive={}", tag_exclusive);

        // tokenize (Sudachi 正規化)
        let tokens = match sudachi_tokenize_large(&query_str, SudachiMode::A, 2000) {
            Ok(t) => t,
            Err(e) => {
                warn!("sudachi_tokenize_large error: {}", e);
                let result = SearchRes::Failed { error: format!("Tokenization error: {}", e) };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(500);
                return c;
            }
        };
        if tokens.is_empty() {
            let result = SearchRes::Success { query: query_str, tokenize_query: tokens, algorithm: algo_str.clone(), range, results: Vec::new() };
            c.res.json_value(&serde_json::to_value(&result).unwrap());
            c.res.set_status(200);
            return c;
        }

        let tf = TokenFrequency::from(&tokens[..]);

        // IndexPool を使ってスコア計算
        let scored = c.c.index_pool.per_similarity(&tf, &algo);
        println!("Scored {} documents", scored.len());
        let sorted = c.c.index_pool.sort_by_score(scored);
        let results = c.c.index_pool.generate_results(sorted, range.clone(), tags, tag_exclusive);
        let result = SearchRes::Success { 
            query: query_str, 
            tokenize_query: tokens, 
            algorithm: algo_str, 
            range: range, 
            results: results 
        };
        c.res.json_value(&serde_json::to_value(&result).unwrap());
        c.res.set_status(200);
        c
    });

    kurosabi.not_found_handler(|mut c| async move {
        c.res.text("Not Found");
        c.res.set_status(404);
        c
    });

    kurosabi
        .server()
        .port(90)
        .host([0,0,0,0])
        .build()
        .run_async()
        .await;
}

fn init_logging() {
    // RUST_LOG が未設定ならデフォルトを与える
    let has_env = std::env::var("RUST_LOG").is_ok();
    if !has_env {
        unsafe { std::env::set_var("RUST_LOG", "debug"); }
    }
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format(|f, record| {
            use chrono::Local;
            let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            writeln!(f, "[{ts}] {lvl} {target}: {msg}",
                lvl = record.level(),
                target = record.target(),
                msg = record.args())
        })
        .filter_level(LevelFilter::Debug)
        .try_init();
}

// 検索アルゴリズムの簡易パーサ
fn parse_algo(s: &str) -> SimilarityAlgorithm {
    let lower = s.trim().to_ascii_lowercase();
    // 補助: 引数の括弧内から数値を抽出
    fn nums(src: &str) -> Vec<f64> {
        if let (Some(l), Some(r)) = (src.find('('), src.rfind(')')) {
            let inner = &src[l + 1..r];
            inner
                .split(',')
                .filter_map(|p| p.trim().parse::<f64>().ok())
                .collect()
        } else {
            Vec::new()
        }
    }

    if lower.starts_with("dot") {
        SimilarityAlgorithm::Dot
    } else if lower.starts_with("cosine") || lower.starts_with("cosinesimilarity") {
        SimilarityAlgorithm::CosineSimilarity
    } else if lower.starts_with("bm25plus") {
        let v = nums(&lower);
        let k1 = v.get(0).copied().unwrap_or(1.2);
        let b = v.get(1).copied().unwrap_or(0.75);
        let delta = v.get(2).copied().unwrap_or(0.5);
        SimilarityAlgorithm::BM25plus(k1, b, delta)
    } else if lower.starts_with("bm25l") {
        let v = nums(&lower);
        let k1 = v.get(0).copied().unwrap_or(1.2);
        let b = v.get(1).copied().unwrap_or(0.75);
        SimilarityAlgorithm::BM25L(k1, b)
    } else if lower.starts_with("bm25cosinenormalizedlinearcombination") {
        let v = nums(&lower);
        let k1 = v.get(0).copied().unwrap_or(1.2);
        let b = v.get(1).copied().unwrap_or(0.75);
        let alpha = v.get(2).copied().unwrap_or(0.5);
        SimilarityAlgorithm::BM25CosineNormalizedLinearCombination(k1, b, alpha)
    } else if lower.starts_with("bm25cosinefilter") {
        let v = nums(&lower);
        let k1 = v.get(0).copied().unwrap_or(1.2);
        let b = v.get(1).copied().unwrap_or(0.75);
        SimilarityAlgorithm::BM25CosineFilter(k1, b)
    } else if lower.starts_with("bm25prfcosinesimilarity") {
        let v = nums(&lower);
        let k1 = v.get(0).copied().unwrap_or(1.2);
        let b = v.get(1).copied().unwrap_or(0.75);
        let top_n = v.get(2).copied().unwrap_or(10.0) as usize;
        let alpha = v.get(3).copied().unwrap_or(0.5);
        SimilarityAlgorithm::BM25PrfCosineSimilarity(k1, b, top_n, alpha)
    } else if lower.starts_with("bm25") {
        let v = nums(&lower);
        let k1 = v.get(0).copied().unwrap_or(1.2);
        let b = v.get(1).copied().unwrap_or(0.75);
        SimilarityAlgorithm::BM25(k1, b)
    } else {
        // 既定
        SimilarityAlgorithm::BM25(1.2, 0.75)
    }
}

// range クエリ文字列を正規化して (start, end) (endは排他的) を返す
// 受け入れる形式:
//   "a..b"  -> a..b
//   "..b"   -> 0..b
//   "a.."   -> a..a+DEFAULT_SEARCH_RESULTS
//   "v"     -> v..v+DEFAULT_SEARCH_RESULTS
//   空/None  -> 0..DEFAULT_SEARCH_RESULTS
// 正規化:
//   1) 解析失敗はデフォルト
//   2) end < start の場合 swap (例: 20..10 -> 10..20)
//   3) 幅 > MAX_SEARCH_RESULTS の場合 end = start + MAX_SEARCH_RESULTS
//   4) 加算は saturating_add でオーバーフロー防止
fn parse_range_param(raw: Option<String>) -> (usize, usize) {
    let default_end = DEFAULT_SEARCH_RESULTS.min(MAX_SEARCH_RESULTS);
    let Some(s) = raw else { return (0, default_end); };
    if s.is_empty() { return (0, default_end); }

    let (mut start, mut end) = if let Some((l, r)) = s.split_once("..") {
        // a..b / a.. / ..b
        let start = if l.is_empty() { 0 } else { l.parse::<usize>().unwrap_or(0) };
        if r.is_empty() {
            // a..  -> a..a+DEFAULT
            let tentative = start.saturating_add(DEFAULT_SEARCH_RESULTS);
            (start, tentative)
        } else {
            // a..b / ..b
            let end = r.parse::<usize>().unwrap_or(start);
            let start = if l.is_empty() { 0 } else { start }; // ..b の場合 start=0
            (start, end)
        }
    } else {
        // 単値 v
        let v = s.parse::<usize>().unwrap_or(0);
        let end = v.saturating_add(DEFAULT_SEARCH_RESULTS);
        (v, end)
    };

    // swap if reversed
    if end < start { std::mem::swap(&mut start, &mut end); }

    // 幅制限
    let max_end = start.saturating_add(MAX_SEARCH_RESULTS);
    if end > max_end { end = max_end; }

    (start, end)
}







