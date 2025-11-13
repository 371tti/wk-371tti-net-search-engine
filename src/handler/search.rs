use kurosabi::kurosabi::Context;
use log::{debug, warn};
use percent_encoding::percent_decode_str;
use tf_idf_vectorizer::TokenFrequency;

use crate::{BLOCK_INDEX_ACCESS, collect::{IndexRes, SearchRes}, context::SearchContext, engine::tag::Tags, utils::{parse_algo, parse_range_param}};

pub struct SearchHandler;

impl SearchHandler {
    pub async fn search(mut c: Context<SearchContext>) -> Context<SearchContext> {
        if BLOCK_INDEX_ACCESS.load(std::sync::atomic::Ordering::SeqCst) {
            warn!("Index access is currently blocked");
            let result = IndexRes::Failed { error: "Index access is currently blocked".to_string() };
            c.res.json_value(&serde_json::to_value(&result).unwrap());
            c.res.set_status(503);
            return c;
        }
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
        let tokens = match c.c.sudachi_tokenizer.mix_query_tokenizer(&query_str) {
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
        let scored = c.c.index_pool.per_similarity(&tf, &algo).await;
        println!("Scored {} documents", scored.len());
        let sorted = c.c.index_pool.sort_by_score(scored);
        let results = c.c.index_pool.generate_results(sorted, range.clone(), tags, tag_exclusive).await;
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
    }
}