use chrono::{DateTime, Utc};
use serde::Serialize;

pub struct ScoredEntry {
    pub score: f64,
    pub key: usize,
    pub length: u64,
    pub index_id: usize,
}

#[derive(Debug, Clone, Serialize)]
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