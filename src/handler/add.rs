use kurosabi::kurosabi::Context;
use log::{info, warn};
use tf_idf_vectorizer::TokenFrequency;

use crate::{BLOCK_INDEX_ACCESS, MAX_DESC_LENGTH, MAX_TITLE_LENGTH, SCRAPER_API_URL, collect::{IndexReq, IndexRes, ScraperResult}, context::SearchContext, engine::{meta::IndexMeta, tag::Tags}, http_client::fetch_scraper_api, utils::percent_encode_query_value};

pub struct AddHandler;

impl AddHandler {
    // URL追加ハンドラ
    pub async fn add(mut c: Context<SearchContext>) -> Context<SearchContext> {
        if BLOCK_INDEX_ACCESS.load(std::sync::atomic::Ordering::SeqCst) {
            warn!("Index access is currently blocked");
            let result = IndexRes::Failed { error: "Index access is currently blocked".to_string() };
            c.res.json_value(&serde_json::to_value(&result).unwrap());
            c.res.set_status(503);
            return c;
        }

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

        // クエリパラメータとフラグメント削除
        let url = {
            let s = &index_req.url;
            // フラグメント削除
            let no_frag = s.split('#').next().unwrap_or(s);

            no_frag.split("?").next().unwrap_or(no_frag).to_string()
        };

        println!("Received /add request for URL: {}", url);
        // url をクエリ値として安全に渡すためエンコード
        let escaped = percent_encode_query_value(&url);
        let mut req_url = format!("{}{}", SCRAPER_API_URL, escaped);
        if let Some(selector) = &index_req.target_selector {
            // セレクタが指定されている場合は追加でパラメータを付与
            req_url.push_str("&text_selector=");
            req_url.push_str(&selector);
        }
        println!("Fetching from scraper API: {}", req_url);
        let scraper_result = match fetch_scraper_api(&req_url).await {
            Ok(res) => res,
            Err(e) => {
                warn!("Failed to fetch scraper API: {}", e);
                let result = IndexRes::Failed { error: format!("Failed to fetch scraper API: {}", e) };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(500);
                return c;
            }
        };
        println!("Scraper API response received for URL: {}", url);

        match scraper_result {
            ScraperResult::Success { results, status,  url: _ } => {
                if status != 200 {
                    warn!("Scraper API returned non-200 status: {}", status);
                    let result = IndexRes::Failed { error: format!("Scraper API returned status: {}", status) };
                    c.res.json_value(&serde_json::to_value(&result).unwrap());
                    c.res.set_status(500);
                    return c;
                }

                let body = results.text;

                let title = match index_req.title.or_else(|| results.title) {
                    Some(t) => t,
                    None => "No Title".to_string(),
                }.chars().take(MAX_TITLE_LENGTH).collect();

                let description = match index_req.descriptions.clone() {
                    Some(d) => d.chars().take(MAX_DESC_LENGTH).collect(),
                    None => body.chars().take(MAX_DESC_LENGTH).collect(), // 先頭500文字を説明に
                };
                
                let favicon: Option<Box<str>> = index_req.favicon.or_else(|| results.favicon ).map(|s| s.into_boxed_str());
                let url = results.url.into_boxed_str();
                let tags = Tags::from_strs(&index_req.tags);
                let lang = results.lang.map(|s| s.clone().into_boxed_str());
                let links = results.links.into_iter().map(|v| v.into_boxed_str()).collect();

                let (tokens, token_sum) = match c.c.sudachi_tokenizer.mix_doc_tokenizer(&body) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("sudachi_tokenize_large error: {}", e);
                        let result = IndexRes::Failed { error: format!("Tokenization error: {}", e) };
                        c.res.json_value(&serde_json::to_value(&result).unwrap());
                        c.res.set_status(500);
                        return c;
                    }
                };

                let meta = IndexMeta { 
                    id: 0, 
                    token_sum,
                    url, 
                    title, 
                    description, 
                    favicon, 
                    lang,
                    time: chrono::Utc::now(), 
                    points: 0.0, 
                    tags,
                    links
                };

                let token_fq = TokenFrequency::from(&tokens[..]);

                let _is_success = c.c.index_pool.add_document(&token_fq, meta.clone()).await;
                info!("Added URL: {}", meta.url);
                let result = IndexRes::Success { 
                    url: meta.url, 
                    title: meta.title, 
                    favicon: meta.favicon, 
                    tags: meta.tags.tags(), 
                    descriptions: meta.description, 
                    links: meta.links.clone(),
                };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(200);
                return c;
            }
            ScraperResult::Failed { error } => {
                warn!("Scraper API returned error: {}", error);
                let result = IndexRes::Failed { error: format!("Scraper API error: {}", error) };
                c.res.json_value(&serde_json::to_value(&result).unwrap());
                c.res.set_status(500);
                return c;
            }
        }
    }
}