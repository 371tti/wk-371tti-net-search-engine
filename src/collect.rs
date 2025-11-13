use std::{collections::HashMap, ops::Range};

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

use crate::engine::meta::IndexMeta;

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
        tokenize_query: Vec<Box<str>>,
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
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub descriptions: Option<String>,
    #[serde(default)]
    pub target_selector: Option<String>,
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
        links: Vec<Box<str>>,
    },
    #[serde(rename = "false")]
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetaReq {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "success")]
pub enum MetaRes {
    #[serde(rename = "true")]
    Success {
        meta: IndexMeta,
    },
    #[serde(rename = "false")]
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResults {
    pub url: String,
    pub title: Option<String>,
    pub contents: HashMap<String, Vec<String>>,
    pub lang: Option<String>,
    pub favicon: Option<String>,
    pub links: Vec<String>,
    pub document: String,
    pub text: String,
}

/// success が bool の API レスポンスに対応 (例: {"success":true, ...} / {"success":false, "error":...})
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "success")]
pub enum ScraperResult {
    #[serde(rename = "true")]
    Success {
        status: u16,
        url: String,
        results: ScrapeResults,
    },
    #[serde(rename = "false")]
    Failed {
        error: String,
    },
}