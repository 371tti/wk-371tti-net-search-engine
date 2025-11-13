use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::engine::tag::Tags;

/// Index の基本情報
/// URL, title, description, favicon, time, points, tags
/// Hash と Equal は URL のみで判定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub id: usize,
    /// token_sum
    pub token_sum: u64,
    /// URL
    /// only URL is used for Hash and Equal
    pub url: Box<str>,
    /// Title
    pub title: Box<str>,
    /// Description
    pub description: Box<str>,
    /// links
    pub links: Vec<Box<str>>,
    /// Favicon URL
    pub favicon: Option<Box<str>>,
    /// language code (e.g. "en", "ja", etc)
    pub lang: Option<Box<str>>,
    /// Upload Time
    pub time: DateTime<Utc>,
    /// Score
    pub points: f64,
    /// Tags
    /// General Tag:
    /// - Wiki: wikipedia, ニコニコ大百科, etc
    /// - News: yahoo!, GIGAZINE, ITmedia, etc
    /// - SNS: twitter, facebook, youtube, instagram, etc
    /// - Blog: hatena, zenn, etc
    /// - Forum: 5ch, reddit, stackoverflow, etc
    /// - Shopping: amazon, rakuten, ebay, etc
    /// - Academic: arxiv, ciNii, etc
    /// - Tools: translate, map, etc
    pub tags: Tags,
}
