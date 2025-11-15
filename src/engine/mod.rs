pub mod meta;
pub mod tag;
pub mod search;
pub mod load;
pub mod save;

use std::io::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, TryLockError};

use log::{error, warn};
use tf_idf_vectorizer::{Corpus, TFIDFVectorizer, TokenFrequency};

use crate::engine::meta::IndexMeta;


pub struct IndexPool {
    pub corpus: Arc<Corpus>,
    /// Index shards
    /// idと対応を絶対強制
    pub indexes: Vec<Arc<RwLock<Index>>>,
    pub index_dir: String,
    pub counter: AtomicU64,
}

pub const DEFAULT_INDEX_SHARD_NUM: usize = 16;
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

    pub async fn get_meta(&self, url: &str) -> Option<IndexMeta> {
        for index in &self.indexes {
            let guard = match index.try_read() {
                Ok(g) => g,
                Err(e) => match e {
                    TryLockError::WouldBlock => {
                        warn!("RwLock would block, skipping");
                        continue;
                    }
                    TryLockError::Poisoned(e) => {
                        error!("RwLock poisoned: {}", e);
                        continue;
                    }
                },
            };
            if let Some(meta) = guard.meta_from_url(url) {
                return Some(meta.clone());
            }
        }
        None
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
    pub async fn add_document(&self,
        token_fq: &TokenFrequency,
        mut meta: IndexMeta,
    ) -> Option<bool> {

        let url = meta.url.clone();

        let do_flags = match self.url_to_entry_hint(&url) {
            EntryHintResult::Error => {
                return None;
            },
            EntryHintResult::New(shard_id) => {
                // 新規登録
                let mut idx = match self.indexes[shard_id].write() {
                    Ok(i) => i,
                    Err(e) => {
                        error!("RwLock poisoned: {}", e);
                        return None;
                    },
                };
                let doc_id = idx.generate_next_id();
                idx.vectorizer.add_doc(doc_id, token_fq);
                // token_sumを強引に調整(複数のtokenizerを通している場合に合わなくなるため これでc_tokensについては正確になる)
                idx.vectorizer.documents.get_mut(doc_id).map(|d| {
                    d.token_sum = meta.token_sum;
                });
                idx.vectorizer.update_idf();
                meta.id = doc_id;
                meta.links.iter_mut().for_each(|link| {
                    let s: &str = link.as_ref();
                    let no_frag = s.split('#').next().unwrap_or(s);
                    let no_query = no_frag.split('?').next().unwrap_or(no_frag);
                    *link = no_query.to_string().into();
                });
                meta.links.sort_unstable();
                meta.links.dedup();
                idx.vectorizer.update_idf();
                idx.meta.push(meta);
                let do_save = idx.update_count % SAVE_FILE_INTERVAL == 0;
                let do_calculate_size = idx.update_count % CALCULATE_BIN_SIZE_INTERVAL == 0;
                idx.update_count += 1;
                self.counter.fetch_add(1, Ordering::SeqCst);
                (do_save, do_calculate_size, shard_id, true)
            },
            EntryHintResult::Existing(shard_id, doc_id) => {
                // 既存を削除してから再登録
                let mut idx = match self.indexes[shard_id].write() {
                    Ok(i) => i,
                    Err(e) => {
                        error!("RwLock poisoned: {}", e);
                        return None;
                    },
                };
                idx.vectorizer.del_doc(&doc_id);
                idx.vectorizer.add_doc(doc_id, token_fq);
                // token_sumを強引に調整(複数のtokenizerを通している場合に合わなくなるため これでc_tokensについては正確になる)
                idx.vectorizer.documents.get_mut(doc_id).map(|d| {
                    d.token_sum = meta.token_sum;
                });
                meta.links.iter_mut().for_each(|link| {
                    let s: &str = link.as_ref();
                    let no_frag = s.split('#').next().unwrap_or(s);
                    let no_query = no_frag.split('?').next().unwrap_or(no_frag);
                    *link = no_query.to_string().into();
                });
                meta.links.sort_unstable();
                meta.links.dedup();
                idx.vectorizer.update_idf();
                idx.meta_from_id_mut(doc_id).map(|m| {
                    m.url = meta.url.clone();
                    m.title = meta.title.clone();
                    m.favicon = meta.favicon.clone();
                    m.tags = meta.tags.clone();
                    m.description = meta.description.clone();
                    m.points = meta.points;
                    m.time = meta.time;
                    m.lang = meta.lang.clone();
                    m.links = meta.links.clone();
                    m.token_sum = meta.token_sum;
                });
                let do_save = idx.update_count % SAVE_FILE_INTERVAL == 0;
                let do_calculate_size = idx.update_count % CALCULATE_BIN_SIZE_INTERVAL == 0;
                idx.update_count += 1;
                (do_save, do_calculate_size, shard_id, false)
            }
        };

        let (do_save, do_calculate_size, shard_id, is_new) = do_flags;
        if do_save {
            // Save the index to disk
            if let Ok(bin_size) = self.save_shard(shard_id, &self.index_dir).await {
                let mut idx = match self.indexes[shard_id].write() {
                    Ok(i) => i,
                    Err(e) => {
                        error!("RwLock poisoned: {}", e);
                        return None;
                    },
                };
                idx.vectorizer_bin_size = bin_size.0;
                idx.meta_bin_size = bin_size.1;
            }
        } else if do_calculate_size {
            // Just calculate the binary size
            if let Ok(bin_size) = self.calculate_shard_size(shard_id).await {
                let mut idx = match self.indexes[shard_id].write() {
                    Ok(i) => i,
                    Err(e) => {
                        error!("RwLock poisoned: {}", e);
                        return None;
                    },
                };
                idx.vectorizer_bin_size = bin_size.0;
                idx.meta_bin_size = bin_size.1;
            }
        }

        Some(is_new)
    }

    pub async fn del_document(&self, url: &str) -> bool {
        let mut found = false;
        let mut shard_id = 0;
        let mut doc_id = 0;
        // 既存で登録されているかチェック
        for index in &self.indexes {
            let idx = match index.read() {
                Ok(i) => i,
                Err(e) => {
                    error!("RwLock poisoned: {}", e);
                    continue;
                }
            };
            if let Some(meta) = idx.meta_from_url(url) {
                shard_id = idx.id;
                doc_id = meta.id;
                found = true;
                break;
            }
        }
        if found {
            let mut idx = match self.indexes[shard_id].write() {
                Ok(i) => i,
                Err(e) => {
                    error!("RwLock poisoned: {}", e);
                    return false;
                },
            };
            idx.vectorizer.del_doc(&doc_id);
            idx.vectorizer.update_idf();
            // metaは先所しない、 削除するロジックにしたら多少ファイルサイズ小さくなるかもだけどlock延長のほうが悪いとおもうので
            // idx.meta.retain(|m| m.id != doc_id);
            idx.update_count += 1;
            self.counter.fetch_sub(1, Ordering::SeqCst);
        }
        found
    }

    pub async fn calculate_shard_size(&self, shard_id: usize) -> Result<(u64, u64), Box<dyn std::error::Error + Send + Sync>> {
        // Just calculate the binary size of the specified shard
        if let Some(entry) = self.indexes.get(shard_id) {
            let index = match entry.read() {
                Ok(i) => i,
                Err(e) => {
                    error!("RwLock poisoned: {}", e);
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, "RwLock poisoned").into());
                },
            };

            let vectorizer_bin_size = bincode::serialized_size(&index.vectorizer)?;
            let meta_bin_size = bincode::serialized_size(&index.meta)?;

            Ok((vectorizer_bin_size, meta_bin_size))
        } else {
            return Err(Box::new(Error::new(std::io::ErrorKind::NotFound, "Index shard not found")));
        }
    }

    pub fn wait_for_writing(&self) {
        for index in &self.indexes {
            match index.write() {
                Ok(_i) => {},
                Err(e) => {
                    error!("RwLock poisoned: {}", e);
                    continue;
                }
            };
        }
    }

    pub fn url_to_entry_hint(&self, url: &str) -> EntryHintResult {
        let mut is_new = true;
        let mut shard_id = 0;
        let mut doc_id = 0;
        // 既存で登録されているかチェック
        // 最小サイズシャード選択用 (初期は最大値)
        let mut best_size: u64 = u64::MAX;
        for index in &self.indexes {
            match index.read() {
                Ok(idx) => {
                    if is_new {
                        if let Some(m) = idx.meta_from_url(&url) {
                            shard_id = idx.id;
                            doc_id = m.id;
                            is_new = false;
                            break;
                        }
                    }
                    // 未登録なら最もサイズの小さいシャードへ
                    let size = idx.meta_bin_size.max(idx.vectorizer_bin_size);
                    if size <= best_size {
                        best_size = size;
                        shard_id = idx.id;
                    }
                }
                Err(_e) => {
                    error!("RwLock poisoned: {}", _e);
                    return EntryHintResult::Error;
                }
            }
        }
        if is_new {
            EntryHintResult::New(shard_id)
        } else {
            EntryHintResult::Existing(shard_id, doc_id)
        }
    }

    pub fn url_to_token_freq(&self, url: &str) -> Option<TokenFrequency> {
        for index in &self.indexes {
            let guard = match index.try_read() {
                Ok(g) => g,
                Err(e) => match e {
                    TryLockError::WouldBlock => {
                        warn!("RwLock would block, skipping");
                        continue;
                    }
                    TryLockError::Poisoned(e) => {
                        error!("RwLock poisoned: {}", e);
                        continue;
                    }
                },
            };
            if let Some(meta) = guard.meta_from_url(url) {
                if let Some(tf) = guard.vectorizer.get_tf_into_token_freq(&meta.id) {
                    return Some(tf.clone());
                }
            }
        }
        None
    }
}

pub enum EntryHintResult {
    New(usize),          // shard_id
    Existing(usize, usize), // shard_id, doc_id
    Error,
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

