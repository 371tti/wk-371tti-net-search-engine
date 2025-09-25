use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, RwLock};
use std::hash::Hash;

use log::warn;
use rayon::{prelude::*, result};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tf_idf_vectorizer::{Corpus, SimilarityAlgorithm, TFIDFData, TFIDFVectorizer, TokenFrequency};
use serde::{Serialize, Deserialize};

use crate::collect::{ResEntry, ScoredEntry, SortEntry};

use super::collect::SimilarityResult;


pub struct IndexPool {
    pub corpus: Arc<Corpus>,
    pub indexes: DashMap<usize, Arc<RwLock<Index>>>,
}

impl IndexPool {
    pub fn new() -> Self {
        Self {
            corpus: Arc::new(Corpus::new()),
            indexes: DashMap::new(),
        }
    }

    /// Calculate similarity for all indexes in parallel
    /// Returns a vector of (Hits<IndexMeta>, usize) tuples
    /// where usize is the index ID
    /// 
    /// # Arguments
    /// * `token_fq` - TokenFrequency to compare against
    /// * `algorithm` - SimilarityAlgorithm to use
    /// 
    /// # Returns
    /// Vector of (Hits<IndexMeta>, usize) tuples
    /// 
    /// # Example
    /// ```
    /// let results = index_pool.per_similarity(&token_fq, &SimilarityAlgorithm::CosineSimilarity);
    /// for (hits, index_id) in results.0 {
    ///     println!("Index ID: {}, Hits: {:?}", index_id, hits);
    /// }
    /// ```
    pub fn per_similarity(&self, token_fq: &TokenFrequency, algorithm: &SimilarityAlgorithm) -> Vec<ScoredEntry> {
        let arcs: Vec<Arc<RwLock<Index>>> = self.indexes
            .iter().map(|e| Arc::clone(e.value())).collect();

        let result: Vec<ScoredEntry> = arcs
            .iter().filter_map(|e| e.try_read().ok())
            .collect::<Vec<_>>()
            .par_iter().flat_map(|idx| {
                let mut result = Vec::new();
                let hits = idx.vectorizer.similarity_uncheck_idf(token_fq, algorithm);
                hits.list.iter().for_each(|h| {
                    result.push(ScoredEntry {
                        score: h.1,
                        key: h.0,
                        length: h.2,
                        index_id: idx.id,
                    });
                });
                result
            }).collect();
        result
    }

    pub fn sort_by_score(&self, mut results: Vec<ScoredEntry>) -> Vec<ScoredEntry> {
        results
            .par_iter_mut()
            .for_each(|h| {
                // Calculate total score for each hit
                h.score = h.score; // Placeholder for actual score calculation if needed
            });
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    pub fn generate_results(&self, results: Vec<ScoredEntry>, range: Range<usize>, tag: Tags, tag_exclusive: bool) -> Vec<ResEntry> {
        let mut res_entries = Vec::new();
        let range_results = &results[range];
        for scored in range_results {
            let index = match self.indexes.get(&scored.index_id) {
                Some(idx) => Arc::clone(idx.value()),
                None => continue,
            };
            let index_read = match index.read() {
                Ok(r) => r,
                Err(_poison) => {
                    warn!("RwLock poisoned for index id {}, skipping", scored.index_id);
                    continue; // Skip poisoned lock
                }
            };
            let meta = match index_read.meta.get(scored.key) {
                Some(m) => m,
                None => continue,
            };
            // タグフィルタリング
            // example: tag_exclusive = true -> 完全一致, false -> 部分一致
            if (tag_exclusive && !meta.tags.is_filter_contains(tag)) || (!tag_exclusive && !meta.tags.contains(tag)) {
                continue;
            }
            res_entries.push(ResEntry {
                url: meta.url.clone(),
                title: meta.title.clone(),
                favicon: meta.favicon.clone(),
                tags: meta.tags.tags(),
                descriptions: meta.description.clone(),
                score: scored.score,
                point: meta.points,
                length: scored.length,
                id: scored.key,
                index_id: scored.index_id,
                time: meta.time,
            });
        }
        res_entries
    }


    /// Load indexes and corpus from the specified directory
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // .corpus
        let corpus_path = std::fs::read_dir(path)?
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        log::error!("Failed to read dir entry: {}", e);
                        return None;
                    }
                };
                let path = entry.path();
                if path.extension()? == "corpus" {
                    Some(path)
                } else {
                    None
                }
            })
            .next()
            .ok_or("No corpus file found")?;

        // N.index (N: usize)
        let index_paths = std::fs::read_dir(path)?
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!("Failed to read dir entry: {}", e);
                        return None;
                    }
                };
                let path = entry.path();
                if path.extension()? == "index" {
                    Some(path)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let meta_paths = std::fs::read_dir(path)?
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!("Failed to read dir entry: {}", e);
                        return None;
                    }
                };
                let path = entry.path();
                if path.extension()? == "meta" {
                    Some(path)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let corpus_data = match std::fs::read(corpus_path.as_path()) {
            Ok(data) => data,
            Err(e) => {
                log::error!("Failed to read corpus file: {}", e);
                return Err(Box::new(e));
            }
        };
        let corpus: Arc<Corpus> = match bincode::deserialize(&corpus_data) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                log::error!("Failed to deserialize corpus: {}", e);
                return Err(Box::new(e));
            }
        };

        let indexes = DashMap::new();

        let vectorizer_map: HashMap<usize, TFIDFVectorizer<u16, usize>> = index_paths.iter()
            .filter_map(|path| {
                let id = path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<usize>().ok())?;
                let data = match std::fs::read(path) {
                    Ok(d) => d,
                    Err(e) => {
                        log::warn!("Failed to read index file {:?}: {}", path, e);
                        return None;
                    }
                };
                let index: TFIDFData<u16, usize> = match bincode::deserialize(&data) {
                    Ok(idx) => idx,
                    Err(e) => {
                        log::warn!("Failed to deserialize index file {:?}: {}", path, e);
                        return None;
                    }
                };
                let vectorizer = index.into_tf_idf_vectorizer(corpus.clone());
                Some((id, vectorizer))
            })
            .collect();

        let mut meta_map: HashMap<usize, Vec<IndexMeta>> = meta_paths.iter()
            .filter_map(|path| {
                let id = path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<usize>().ok())?;
                let data = match std::fs::read(path) {
                    Ok(d) => d,
                    Err(e) => {
                        log::warn!("Failed to read meta file {:?}: {}", path, e);
                        return None;
                    }
                };
                let meta: Vec<IndexMeta> = match bincode::deserialize(&data) {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!("Failed to deserialize meta file {:?}: {}", path, e);
                        return None;
                    }
                };
                Some((id, meta))
            })
            .collect();

        for (id, vectorizer) in vectorizer_map {
            let meta = match meta_map.remove(&id) {
                Some(m) => m,
                None => {
                    log::warn!("Meta file for index ID {} not found, skip loading", id);
                    continue;
                }
            };
            let index = Index::with_vectorizer(id, vectorizer, meta);
            let index_rwlock_arc = Arc::new(RwLock::new(index));
            indexes.insert(id, index_rwlock_arc);
        }

        Ok(Self { corpus, indexes })
    }

    /// Save indexes and corpus to the specified directory
    pub fn save(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(path)?;

        // Save corpus
        let corpus_path = std::path::Path::new(path).join("global.corpus");
        let corpus_data = bincode::serialize(&*self.corpus)?;
        std::fs::write(corpus_path, corpus_data)?;

        // Save each index and meta
        for entry in self.indexes.iter() {
            let index = entry.value().read().unwrap_or_else(|poison| {
                warn!("RwLock poisoned for index id {}, recovering", entry.key());
                poison.into_inner() // poison を無視して中身を取得
            });
            let index_path = std::path::Path::new(path).join(format!("{}.index", index.id));
            let meta_path = std::path::Path::new(path).join(format!("{}.meta", index.id));

            let index_data = bincode::serialize(&index.vectorizer)?;
            std::fs::write(index_path, index_data)?;

            let meta_data = bincode::serialize(&index.meta)?;
            std::fs::write(meta_path, meta_data)?;
        }

        Ok(())
    }
}

pub const INDEX_VERSION: u32 = 1;

pub struct Index {
    pub id: usize,
    /// TF-IDF Vectorizer
    /// u16: token ID
    /// usize: document ID (= index in meta)
    pub vectorizer: TFIDFVectorizer<u16, usize>,
    /// Metadata for each document
    /// The index in this vector corresponds to the document ID in the vectorizer
    pub meta: Vec<IndexMeta>,
    pub version: u32, // INDEX_VERSION
}

impl Index {
    pub fn new(id: usize, corpus: Arc<Corpus>) -> Self {
        Self {
            id,
            vectorizer: TFIDFVectorizer::<u16, usize>::new(corpus),
            meta: Vec::new(),
            version: INDEX_VERSION,
        }
    }

    pub fn with_vectorizer(id: usize, vectorizer: TFIDFVectorizer<u16, usize>, meta: Vec<IndexMeta>) -> Self {
        Self {
            id,
            vectorizer,
            meta,
            version: INDEX_VERSION,
        }
    }
}

/// Index の基本情報
/// URL, title, description, favicon, time, points, tags
/// Hash と Equal は URL のみで判定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    /// URL
    /// only URL is used for Hash and Equal
    pub url: Box<str>,
    /// Title
    pub title: Box<str>,
    /// Description
    pub description: Box<str>,
    /// Favicon URL
    pub favicon: Option<Box<str>>,
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
    /// - Others: uncategorized
    pub tags: Tags,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Tags(u64);

impl Tags {
    pub const WIKI: u64 = 1 << 0;
    pub const NEWS: u64 = 1 << 1;
    pub const SNS: u64 = 1 << 2;
    pub const BLOG: u64 = 1 << 3;
    pub const FORUM: u64 = 1 << 4;
    pub const SHOPPING: u64 = 1 << 5;
    pub const ACADEMIC: u64 = 1 << 6;
    pub const TOOLS: u64 = 1 << 7;

    /// eg: Tags::new(Tags::NEWS | Tags::BLOG)
    pub fn new(set: u64) -> Self {
        Self(set)
    }

    /// 完全一致するか
    pub fn is_filter_contains<T: Into<u64>>(&self, tag: T) -> bool {
        (self.0 ^ tag.into()) == 0
    }

    pub fn contains<T: Into<u64>>(&self, tag: T) -> bool {
        (self.0 & tag.into()) != 0
    }

    pub fn tags(&self) -> Vec<Box<str>> {
        let mut result = Vec::new();
        if self.contains(Self::WIKI) { result.push("WIKI".into()); }
        if self.contains(Self::NEWS) { result.push("NEWS".into()); }
        if self.contains(Self::SNS) { result.push("SNS".into()); }
        if self.contains(Self::BLOG) { result.push("BLOG".into()); }
        if self.contains(Self::FORUM) { result.push("FORUM".into()); }
        if self.contains(Self::SHOPPING) { result.push("SHOPPING".into()); }
        if self.contains(Self::ACADEMIC) { result.push("ACADEMIC".into()); }
        if self.contains(Self::TOOLS) { result.push("TOOLS".into()); }
        result
    }
}

impl PartialEq for IndexMeta {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
    }
}

impl Into<u64> for Tags {
    fn into(self) -> u64 {
        self.0
    }
}