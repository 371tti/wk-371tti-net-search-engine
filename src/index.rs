use std::collections::HashMap;
use std::io::Error;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use log::{error, warn};
use rayon::prelude::*;
use chrono::{DateTime, Utc};
use tf_idf_vectorizer::{Corpus, SimilarityAlgorithm, TFIDFData, TFIDFVectorizer, TokenFrequency};
use serde::{Serialize, Deserialize};

use crate::collect::{ResEntry, ScoredEntry};


pub struct IndexPool {
    pub corpus: Arc<Corpus>,
    /// Index shards
    /// idと対応を絶対強制
    pub indexes: Vec<Arc<RwLock<Index>>>,
    pub index_dir: String,
    pub counter: AtomicU64,
}

pub const DEFAULT_INDEX_SHARD_NUM: usize = 16;
pub const MAX_FILE_SIZE: usize = 200 * 1024 * 1024; // 200MB
pub const CALCULATE_BIN_SIZE_INTERVAL: usize = 20; // 20回更新ごとにバイナリサイズを再計算
pub const SAVE_FILE_INTERVAL: usize = 100; // 100回更新ごとにディスクに保存

impl IndexPool {
    pub fn new(index_dir: &str) -> Self {
        let corpus = Arc::new(Corpus::new());
        // Create index shards
        let indexes: Vec<Arc<RwLock<Index>>> = (0..DEFAULT_INDEX_SHARD_NUM).map(|i| {
            Arc::new(RwLock::new(Index::new(i, Arc::clone(&corpus))))
        }).collect();
        Self {
            corpus,
            indexes,
            index_dir: index_dir.to_string(),
            counter: AtomicU64::new(0),
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
        let result: Vec<ScoredEntry> = self.indexes
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

    /// Generate ResEntry from ScoredEntry
    /// # Arguments
    /// * `results` - The scored entries to generate results from
    /// * `range` - The range of results to include
    /// * `tag` - The tag to filter results by
    /// * `tag_exclusive` - Whether to use exclusive tag filtering
    /// # Returns
    /// Vector of ResEntry
    pub fn generate_results(&self, results: Vec<ScoredEntry>, range: Range<usize>, tag: Tags, tag_exclusive: bool) -> Vec<ResEntry> {
        let mut res_entries = Vec::new();
        let range_results: &[ScoredEntry] = {
            let len = results.len();
            let start = range.start.min(len);
            let end = range.end.min(len);
            if start >= end {
            &[]
            } else {
            &results[start..end]
            }
        };
        for scored in range_results {
            let index = match self.indexes.get(scored.index_id) {
                Some(idx) => idx,
                None => continue,
            };
            let index_read = match index.read() {
                Ok(r) => r,
                Err(_poison) => {
                    warn!("RwLock poisoned for index id {}, skipping", scored.index_id);
                    continue; // Skip poisoned lock
                }
            };
            let meta = match index_read.meta_from_id(scored.key) {
                Some(m) => m,
                None => continue,
            };
            // タグフィルタリング
            // example: tag_exclusive = true -> 完全一致, false -> 部分一致
            // タグ指定が空でなければフィルタ
            if !tag.is_empty() {
                if tag_exclusive {
                    if !meta.tags.is_filter_contains(tag) {
                        continue;
                    }
                } else {
                    if !meta.tags.contains(tag) {
                        continue;
                    }
                }
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

    /// add document to index pool
    /// meta.id は上書きされます
    /// # Arguments
    /// * `token_fq` - TokenFrequency of the document
    /// * `meta` - Metadata of the document
    /// # Returns
    /// Some(bool) true is new, false is update, None if failed
    /// CALCULATE_BIN_SIZE_INTERVAL ごとにバイナリサイズを再計算し、SAVE_FILE_INTERVAL ごとにディスクに保存します
    /// もっとも負荷の低いシャードに追加されます
    /// 既存のURLがあれば上書きされます
    pub fn add_document(&self,
        token_fq: &TokenFrequency,
        mut meta: IndexMeta,
    ) -> Option<bool> {
        let url = meta.url.clone();
        let mut is_new = true;
        let mut shard_id = 0;
        let mut doc_id = 0;
        // 既存で登録されているかチェック
        let mut max_size = 0;
        for index in &self.indexes {
            match index.read() {
                Ok(idx) => {
                    if is_new {
                        if let Some(meta) = idx.meta_from_url(&url) {
                            // 既に登録されている
                            shard_id = idx.id;
                            doc_id = meta.id;
                            is_new = false;
                            break;
                        }
                    }
                    // 見つからない場合もっとも負荷の低いシャードを選んでく
                    let size = idx.meta_bin_size.max(idx.vectorizer_bin_size);
                    if max_size <= size {
                        shard_id = idx.id;
                        max_size = max_size;
                    }
                }
                Err(_poison) => {
                    warn!("RwLock poisoned, skipping");
                    continue; // Skip poisoned lock
                }
            }
        }
        let do_save;
        let do_calculate_size ;
        if is_new {
            // 新規登録
            if let Ok(mut idx) = self.indexes[shard_id].write() {
                doc_id = idx.generate_next_id();
                idx.vectorizer.add_doc(doc_id, token_fq);
                idx.vectorizer.update_idf();
                meta.id = doc_id;
                idx.meta.push(meta);
                do_save = idx.update_count % SAVE_FILE_INTERVAL == 0;
                do_calculate_size = idx.update_count % CALCULATE_BIN_SIZE_INTERVAL == 0;
                idx.update_count += 1;
                self.counter.fetch_add(1, Ordering::SeqCst);
            } else {
                error!("RwLock poisoned for index id {}, skipping", shard_id);
                return None;
            }
        } else {
            // 既存を削除してから再登録
            if let Ok(mut idx) = self.indexes[shard_id].write() {
                idx.vectorizer.del_doc(&doc_id);
                idx.vectorizer.add_doc(doc_id, token_fq);
                idx.vectorizer.update_idf();
                idx.meta_from_id_mut(doc_id).map(|m| {
                    m.url = meta.url.clone();
                    m.title = meta.title.clone();
                    m.favicon = meta.favicon.clone();
                    m.tags = meta.tags.clone();
                    m.description = meta.description.clone();
                    m.points = meta.points;
                    m.time = meta.time;
                });
                do_save = idx.update_count % SAVE_FILE_INTERVAL == 0;
                do_calculate_size = idx.update_count % CALCULATE_BIN_SIZE_INTERVAL == 0;
                idx.update_count += 1;
            } else {
                error!("RwLock poisoned for index id {}, skipping", shard_id);
                return None;
            }
        }

        if do_save {
            // Save the index to disk
            if let Ok(bin_size) = self.save_shard(shard_id, &self.index_dir) {
                if let Ok(mut idx) = self.indexes[shard_id].write() {
                    idx.vectorizer_bin_size = bin_size.0;
                    idx.meta_bin_size = bin_size.1;
                }
            }
        } else if do_calculate_size {
            // Just calculate the binary size
            if let Ok(bin_size) = self.calculate_shard_size(shard_id) {
                if let Ok(mut idx) = self.indexes[shard_id].write() {
                    idx.vectorizer_bin_size = bin_size.0;
                    idx.meta_bin_size = bin_size.1;
                }
            }
        }

        Some(is_new)
    }

    pub fn del_document(&self, url: &str) -> bool {
        let mut found = false;
        let mut shard_id = 0;
        let mut doc_id = 0;
        // 既存で登録されているかチェック
        for index in &self.indexes {
            match index.read() {
                Ok(idx) => {
                    if let Some(meta) = idx.meta_from_url(url) {
                        // 既に登録されている
                        shard_id = idx.id;
                        doc_id = meta.id;
                        found = true;
                        break;
                    }
                }
                Err(_poison) => {
                    warn!("RwLock poisoned, skipping");
                    continue; // Skip poisoned lock
                }
            }
        }
        if found {
            if let Ok(mut idx) = self.indexes[shard_id].write() {
                idx.vectorizer.del_doc(&doc_id);
                idx.vectorizer.update_idf();
                // metaは先所しない、 削除するロジックにしたら多少ファイルサイズ小さくなるかもだけどlock延長のほうが悪いとおもうので
                // idx.meta.retain(|m| m.id != doc_id);
                idx.update_count += 1;
                self.counter.fetch_sub(1, Ordering::SeqCst);
            } else {
                error!("RwLock poisoned for index id {}, skipping", shard_id);
                return false;
            }
        }
        found
    }

    /// Load indexes and corpus from the specified directory
    /// if not found corpus, create new instance
    pub fn load_or_new(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match Self::load(path) {
            Ok(pool) => Ok(pool),
            Err(e) => {
                warn!("Failed to load index pool from {}: {}, creating new instance", path, e);
                Ok(Self::new(path))
            }
        }
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

        let mut vectorizer_map: HashMap<usize, TFIDFVectorizer<u16, usize>> = index_paths.iter()
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

        let mut indexes = Vec::with_capacity(DEFAULT_INDEX_SHARD_NUM);

        let mut counter: u64 = 0;

        for i in 0..DEFAULT_INDEX_SHARD_NUM {
            let vectorizer = vectorizer_map.remove(&i).ok_or_else(|| {
                log::error!("No vectorizer found for index id {}", i);
                Box::new(Error::new(std::io::ErrorKind::NotFound, "Vectorizer not found"))
            })?;
            counter += vectorizer.doc_num() as u64;
            let vectorizer_bin_size = bincode::serialized_size(&vectorizer)?;
            let meta = meta_map.remove(&i).ok_or_else(|| {
                log::error!("No meta found for index id {}", i);
                Box::new(Error::new(std::io::ErrorKind::NotFound, "Meta not found"))
            })?;
            let meta_bin_size = bincode::serialized_size(&meta)?;
            indexes.push(Arc::new(RwLock::new(Index::with_vectorizer(i, vectorizer, meta, vectorizer_bin_size, meta_bin_size))));
        }

        Ok(Self { corpus, indexes, index_dir: path.to_string(), counter: AtomicU64::new(counter) })
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
            let index = entry.read().map_err(|e| {
                log::error!("Failed to acquire read lock for index: {}", e);
                Box::new(Error::new(std::io::ErrorKind::Other, "RwLock poisoned"))
            })?;
            let index_path = std::path::Path::new(path).join(format!("{}.index", index.id));
            let meta_path = std::path::Path::new(path).join(format!("{}.meta", index.id));

            let index_data = bincode::serialize(&index.vectorizer)?;
            std::fs::write(index_path, index_data)?;

            let meta_data = bincode::serialize(&index.meta)?;
            std::fs::write(meta_path, meta_data)?;
        }

        Ok(())
    }

    /// 指定したシャードのみ上書き保存
    /// # Arguments
    /// * `shard_id` - シャードID
    /// * `path` - 保存先ディレクトリ
    /// # Returns
    /// Ok((u64, u64)) or Err
    /// u64: vectorizer size, u64: meta size
    pub fn save_shard(&self, shard_id: usize, path: &str) -> Result<(u64, u64), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(path)?;

        // Save corpus
        let corpus_path = std::path::Path::new(path).join("global.corpus");
        let corpus_file = std::fs::File::create(&corpus_path)?;
        let mut corpus_writer = std::io::BufWriter::new(corpus_file);
        bincode::serialize_into(&mut corpus_writer, &*self.corpus)?;

        // Save specified index and meta
        if let Some(entry) = self.indexes.get(shard_id) {
            let index = entry.read().map_err(|e| {
                log::error!("Failed to acquire read lock for index: {}", e);
                Box::new(Error::new(std::io::ErrorKind::Other, "RwLock poisoned"))
            })?;

            // Save vectorizer
            let index_path = std::path::Path::new(path).join(format!("{}.index", index.id));
            let index_file = std::fs::File::create(&index_path)?;
            let mut index_writer = std::io::BufWriter::new(index_file);
            bincode::serialize_into(&mut index_writer, &index.vectorizer)?;

            // Save metadata
            let meta_path = std::path::Path::new(path).join(format!("{}.meta", index.id));
            let meta_file = std::fs::File::create(&meta_path)?;
            let mut meta_writer = std::io::BufWriter::new(meta_file);
            bincode::serialize_into(&mut meta_writer, &index.meta)?;

            // Get file sizes
            let vectorizer_bin_size = std::fs::metadata(&index_path)?.len();
            let meta_bin_size = std::fs::metadata(&meta_path)?.len();

            Ok((vectorizer_bin_size, meta_bin_size))
        } else {
            return Err(Box::new(Error::new(std::io::ErrorKind::NotFound, "Index shard not found")));
        }
    }

    pub fn calculate_shard_size(&self, shard_id: usize) -> Result<(u64, u64), Box<dyn std::error::Error>> {
        // Just calculate the binary size of the specified shard
        if let Some(entry) = self.indexes.get(shard_id) {
            let index = entry.read().map_err(|e| {
                log::error!("Failed to acquire read lock for index: {}", e);
                Box::new(Error::new(std::io::ErrorKind::Other, "RwLock poisoned"))
            })?;

            let vectorizer_bin_size = bincode::serialized_size(&index.vectorizer)?;
            let meta_bin_size = bincode::serialized_size(&index.meta)?;

            Ok((vectorizer_bin_size, meta_bin_size))
        } else {
            return Err(Box::new(Error::new(std::io::ErrorKind::NotFound, "Index shard not found")));
        }
    }
}


pub struct Index {
    pub id: usize,
    /// TF-IDF Vectorizer
    /// u16: token ID
    /// usize: document ID (= index in meta)
    pub vectorizer: TFIDFVectorizer<u16, usize>,
    /// Metadata for each document
    /// The index in this vector corresponds to the document ID in the vectorizer
    pub meta: Vec<IndexMeta>,
    pub update_count: usize,
    pub vectorizer_bin_size: u64,
    pub meta_bin_size: u64,
}

impl Index {
    pub fn new(id: usize, corpus: Arc<Corpus>) -> Self {
        Self {
            id,
            vectorizer: TFIDFVectorizer::<u16, usize>::new(corpus),
            meta: Vec::new(),
            update_count: 0,
            vectorizer_bin_size: 0,
            meta_bin_size: 0,
        }
    }

    pub fn with_vectorizer(id: usize, vectorizer: TFIDFVectorizer<u16, usize>, meta: Vec<IndexMeta>, vectorizer_bin_size: u64, meta_bin_size: u64) -> Self {
        Self {
            id,
            vectorizer,
            meta,
            update_count: 0,
            vectorizer_bin_size,
            meta_bin_size,
        }
    }

    pub fn meta_from_url(&self, url: &str) -> Option<&IndexMeta> {
        self.meta.iter().find(|m| m.url.as_ref() == url)
    }

    /// idからメタを取得
    /// indexで取得してでなければiter rev で探索
    pub fn meta_from_id(&self, id: usize) -> Option<&IndexMeta> {
        if id > self.meta.len() {
            return None;
        }


        // `id` 以降の要素は存在しないため、`id` の位置から逆方向に探索
        let skip_count = self.meta.len().saturating_sub(id + 1);
        self.meta.iter().rev().skip(skip_count).find(|m| m.id == id)
    }

    pub fn meta_from_id_mut(&mut self, id: usize) -> Option<&mut IndexMeta> {
        if id > self.meta.len() {
            return None;
        }

        // `id` 以降の要素は存在しないため、`id` の位置から逆方向に探索
        let skip_count = self.meta.len().saturating_sub(id + 1);
        self.meta.iter_mut().rev().skip(skip_count).find(|m| m.id == id)
    }

    pub fn generate_next_id(&self) -> usize {
        self.meta.last().and_then(|m| Some(m.id + 1)).unwrap_or(0)
    }
}

/// Index の基本情報
/// URL, title, description, favicon, time, points, tags
/// Hash と Equal は URL のみで判定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub id: usize,
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

    /// すべて満たしてるか
    /// tagはselfに含まれている必要がある
    /// eg: is_filter_contains(Tags::NEWS | Tags::BLOG) -> NEWSとBLOGの両方を含む場合のみtrue
    pub fn is_filter_contains<T: Into<u64> + Copy>(&self, tag: T) -> bool {
        (self.0 & tag.into()) == tag.into()
    }

    pub fn contains<T: Into<u64>>(&self, tag: T) -> bool {
        (self.0 & tag.into()) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
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

    pub fn from_strs<T>(tags: &[T]) -> Self
    where
        T: AsRef<str>,
    {
        let mut set = 0;
        for tag in tags {
            let s = tag.as_ref();
            if s.eq_ignore_ascii_case("wiki")      { set |= Self::WIKI; }
            else if s.eq_ignore_ascii_case("news") { set |= Self::NEWS; }
            else if s.eq_ignore_ascii_case("sns")  { set |= Self::SNS; }
            else if s.eq_ignore_ascii_case("blog") { set |= Self::BLOG; }
            else if s.eq_ignore_ascii_case("forum"){ set |= Self::FORUM; }
            else if s.eq_ignore_ascii_case("shopping"){ set |= Self::SHOPPING; }
            else if s.eq_ignore_ascii_case("academic"){ set |= Self::ACADEMIC; }
            else if s.eq_ignore_ascii_case("tools"){ set |= Self::TOOLS; }
        }
        Self(set)
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