use std::ops::Range;

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

pub struct ScoredEntry {
    pub score: f64,
    pub key: usize,
    pub length: u64,
    pub index_id: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResEntry {
    pub url: Box<str>,
    pub title: Box<str>,
    pub favicon: Option<Box<str>>,
    pub tags: Vec<Box<str>>,
    pub descriptions: Box<str>,
    pub score: f64,
    pub point: f64,
    pub length: u64,
    pub id: usize,
    pub index_id: usize,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "success")]
pub enum SearchRes {
    #[serde(rename = "true")]
    Success {
        query: String,
        tokenize_query: Vec<String>,
        algorithm: String,
        range: Range<usize>,
        results: Vec<ResEntry>,
    },
    #[serde(rename = "false")]
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexReq {
    pub url: String,
    pub title: Option<String>,
    pub favicon: Option<String>,
    /// タグは空でも良い
    /// 例: ["wiki", "blog"]
    /// 使用可能なタグ:
    /// - "wiki": ウィキペディアなどの百科事典
    /// - "news": ニュースサイト
    /// - "sns": ソーシャルメディア
    /// - "blog": ブログ
    /// - "forum": フォーラム
    /// - "shopping": ショッピングサイト
    /// - "academic": 学術論文
    /// - "tools": ツール系サイト
    pub tags: Vec<String>,
    pub descriptions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "success")]
pub enum IndexRes {
    #[serde(rename = "true")]
    Success {
        url: Box<str>,
        title: Box<str>,
        favicon: Option<Box<str>>,
        tags: Vec<Box<str>>,
        descriptions: Box<str>,
    },
    #[serde(rename = "false")]
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResults {
    pub author: Vec<String>,
    pub base: Vec<String>,
    pub canonical: Vec<String>,
    pub content_html: Vec<String>,
    pub descriptions: Vec<String>,
    pub favicon: Vec<String>,
    pub headings: Vec<String>,
    pub lang: Vec<String>,
    pub links: Vec<String>,
    pub modified: Vec<String>,
    pub next: Vec<String>,
    pub prev: Vec<String>,
    pub published: Vec<String>,
    pub rss: Vec<String>,
    pub site_name: Vec<String>,
    pub tags: Vec<String>,
    pub title: Vec<String>,
}

/// success が bool の API レスポンスに対応 (例: {"success":true, ...} / {"success":false, "error":...})
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScraperResult {
    Success {
        success: bool, // 常に true を想定
        status: u16,
        url: String,
        results: ScrapeResults,
    },
    Failed {
        success: bool, // 常に false を想定
        error: String,
    },
}