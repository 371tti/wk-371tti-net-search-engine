mod tokenize;
mod http_client;
mod index;
mod collect;

use kurosabi::Kurosabi;
use serde_json::json;
use tf_idf_vectorizer::{SimilarityAlgorithm, TokenFrequency};
use tracing::{debug, info, warn};
use percent_encoding::percent_decode_str;


pub mod context;
use crate::{collect::sudachi_tokenize_large, context::{SearchContext, SiteMeta}, http_client::fetch_description_and_url};

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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
    info!("Logger initialized");
    let mut kurosabi = Kurosabi::with_context(SearchContext::new());

    kurosabi.get("/add/*", |mut c| async move {
        // 完全なパスから /add/ の後ろ部分を抽出
        let full_path = &c.req.path.path;
        let add_part = if let Some(idx) = full_path.find("/add/") {
            &full_path[idx + 5..]
        } else {
            full_path
        };

    let (descs, url, title, favicon) = match fetch_description_and_url(&format!("http://localhost:88/url/{}", add_part)).await {
            Ok(res) => res,
            Err(e) => {
                warn!("fetch_description_and_url error: {}", e);
                c.res.json_value(&json!({"status": "error", "message": format!("{}", e)}));
                c.res.set_status(500);
                return c;
            }
        };

        // 既存ドキュメントを削除（同一 write ロック内で原子的に）。
        // return c をロックのスコープ外に出して borrow 問題を回避。
        let mut write_lock_err: Option<String> = None;
        {
            match c.c.vectorizer.write() {
                Ok(mut v) => {
                    if v.contains_doc(&url) {
                        v.del_doc(&url);
                    }
                }
                Err(e) => {
                    write_lock_err = Some(e.to_string());
                }
            }
        }
        if let Some(e) = write_lock_err {
            warn!("Failed to acquire vectorizer write lock: {}", e);
            c.res.json_value(&json!({"status": "error", "message": "Failed to acquire vectorizer write lock"}));
            c.res.set_status(500);
            return c;
        }

        let desc = match descs.first() {
            Some(d) => d,
            None => {
                warn!("No description found");
                c.res.json_value(&json!({"status": "error", "message": "No description found"}));
                c.res.set_status(404);
                return c;
            }
        };

        let tokens = match sudachi_tokenize_large(desc, collect::SudachiMode::A, 2000) {
            Ok(t) => t,
            Err(e) => {
                warn!("sudachi_tokenize_large error: {}", e);
                c.res.json_value(&json!({"status": "error", "message": format!("{}", e)}));
                c.res.set_status(500);
                return c;
            }
        };

        let token_freq = TokenFrequency::from(&tokens[..]);

        if let Ok(mut vectorizer) = c.c.vectorizer.write() {
            vectorizer.add_doc(url.clone(), &token_freq);
            vectorizer.update_idf();
            info!("Added URL: {}", url);
            // メタ保存（先頭100文字にトリム）
            let title_trim = title.as_deref().map(|s| SearchContext::trim100(s)).unwrap_or_default();
            let desc_trim = SearchContext::trim100(desc);
            let favicon_val = favicon.clone();
            c.c.meta.insert(url.clone(), SiteMeta { title: title_trim, description: desc_trim, favicon: favicon_val });
            c.res.json_value(&json!({
                "status": "ok",
                "url": url,
                "title": title,
                "favicon": favicon,
                "tokens": tokens.len()
            }));
        } else {
            warn!("Failed to acquire vectorizer lock");
            c.res.json_value(&json!({"status": "error", "message": "Failed to acquire vectorizer lock"}));
            c.res.set_status(500);
        }
        c
    });

    kurosabi.get("/status", |mut c| async move {
        match c.c.vectorizer.read() {
            Ok(v) => {
                let doc_num = v.doc_num();
                debug!("Document count: {}", doc_num);
                c.res.json_value(&json!({"status": "ok", "documents": doc_num}));
            }
            Err(_) => {
                warn!("Failed to acquire vectorizer lock");
                c.res.json_value(&json!({"status": "error", "message": "Failed to acquire vectorizer lock"}));
                c.res.set_status(500);
            }
        }
        c
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
                    c.res.json_value(&json!({"status": "error", "message": "missing query"}));
                    c.res.set_status(400);
                    return c;
                }
                trimmed
            }
            None => {
                c.res.json_value(&json!({"status": "error", "message": "missing query"}));
                c.res.set_status(400);
                return c;
            }
        };
        // top_k
        let top_k: usize = c
            .req
            .path
            .get_query("top_k")
            .and_then(|s| s.parse::<usize>().ok())
            .map(|v| v.clamp(1, 100))
            .unwrap_or(10);
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

        // tokenize (Sudachi 正規化)
        let tokens = match sudachi_tokenize_large(&query_str, collect::SudachiMode::A, 2000) {
            Ok(t) => t,
            Err(e) => {
                warn!("sudachi_tokenize_large error: {}", e);
                c.res.json_value(&json!({"status": "error", "message": format!("{}", e)}));
                c.res.set_status(500);
                return c;
            }
        };
        if tokens.is_empty() {
            c.res.json_value(&json!({
                "status": "ok",
                "query": query_str,
                "algo": algo_str,
                "top_k": top_k,
                "tokens": tokens,
                "results": []
            }));
            return c;
        }

        let tf = TokenFrequency::from(&tokens[..]);

        match c.c.vectorizer.read() {
            Ok(v) => {
                let mut hits = v.similarity_uncheck_idf(&tf, &algo);
                hits.sort_by_score();
                let meta_map = c.c.meta.clone();
                let results: Vec<_> = hits
                    .list
                    .into_iter()
                    .take(top_k)
                    .map(|(id, score, len)| {
                        let (title, description, favicon) = if let Some(m) = meta_map.get(&id) {
                            (Some(m.title.clone()), Some(m.description.clone()), m.favicon.clone())
                        } else {
                            (None, None, None)
                        };
                        json!({
                            "url": id,
                            "score": score,
                            "length": len,
                            "title": title,
                            "description": description,
                            "favicon": favicon
                        })
                    })
                    .collect();
                c.res.json_value(&json!({
                    "status": "ok",
                    "query": query_str,
                    "algo": algo_str,
                    "top_k": top_k,
                    "tokens": tokens,
                    "results": results
                }));
            }
            Err(_) => {
                warn!("Failed to acquire vectorizer lock");
                c.res.json_value(&json!({"status": "error", "message": "Failed to acquire vectorizer lock"}));
                c.res.set_status(500);
            }
        }
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











