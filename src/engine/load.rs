use std::{collections::HashMap, io::Error, sync::{Arc, RwLock, atomic::AtomicU64}};

use log::{info, warn};
use tf_idf_vectorizer::{Corpus, TFIDFData, TFIDFVectorizer};

use crate::engine::{DEFAULT_INDEX_SHARD_NUM, Index, IndexPool, meta::IndexMeta};

impl IndexPool {
    /// Load indexes and corpus from the specified directory
    /// if not found corpus, create new instance
    pub fn load_or_new(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        match Self::load(path) {
            Ok(pool) => Ok(pool),
            Err(e) => {
                warn!("Failed to load index pool from {}: {}, creating new instance", path, e);
                Ok(Self::new(path))
            }
        }
    }

    /// Load indexes and corpus from the specified directory
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
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

        info!("Loading corpus from {:?}", corpus_path);
        let corpus_data = match std::fs::read(corpus_path.as_path()) {
            Ok(data) => data,
            Err(e) => {
                log::error!("Failed to read corpus file: {}", e);
                return Err(Box::new(e));
            }
        };
        info!("Deserializing corpus");
        let corpus: Arc<Corpus> = match bincode::deserialize(&corpus_data) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                log::error!("Failed to deserialize corpus: {}", e);
                return Err(Box::new(e));
            }
        };
        info!("=Done deserializing corpus=\n");

        info!("Loading vectorizers from {} index files", index_paths.len());
        let mut vectorizer_map: HashMap<usize, TFIDFVectorizer<u16, usize>> = index_paths.iter()
            .filter_map(|path| {
                let id = path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<usize>().ok())?;
                info!("Loading vectorizer for index id {}", id);
                let data = match std::fs::read(path) {
                    Ok(d) => d,
                    Err(e) => {
                        log::warn!("Failed to read index file {:?}: {}", path, e);
                        return None;
                    }
                };
                info!("Deserializing vectorizer for index id {}", id);
                let index: TFIDFData<u16, usize> = match bincode::deserialize(&data) {
                    Ok(idx) => idx,
                    Err(e) => {
                        log::warn!("Failed to deserialize index file {:?}: {}", path, e);
                        return None;
                    }
                };
                info!("Done deserializing vectorizer for index id {}", id);
                info!("Linking vectorizer to corpus for index id {}", id);
                let vectorizer = index.into_tf_idf_vectorizer(corpus.clone());
                info!("Done linking vectorizer to corpus for index id {}", id);
                Some((id, vectorizer))
            })
            .collect();
        info!("=Done loading and deserializing all vectorizers=\n");

        info!("Loading metadata from {} meta files", meta_paths.len());
        let mut meta_map: HashMap<usize, Vec<IndexMeta>> = meta_paths.iter()
            .filter_map(|path| {
                let id = path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<usize>().ok())?;
                info!("Loading metadata for index id {}", id);
                let data = match std::fs::read(path) {
                    Ok(d) => d,
                    Err(e) => {
                        log::warn!("Failed to read meta file {:?}: {}", path, e);
                        return None;
                    }
                };
                info!("Deserializing metadata for index id {}", id);
                let meta: Vec<IndexMeta> = match bincode::deserialize(&data) {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!("Failed to deserialize meta file {:?}: {}", path, e);
                        return None;
                    }
                };
                info!("Done deserializing metadata for index id {}", id);
                Some((id, meta))
            })
            .collect();
        info!("=Done loading and deserializing all metadata=\n");

        let mut indexes = Vec::with_capacity(DEFAULT_INDEX_SHARD_NUM);

        let mut counter: u64 = 0;
        info!("Preparing indexes");
        for i in 0..DEFAULT_INDEX_SHARD_NUM {
            info!("Preparing index shard {}", i);
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
            info!("Done preparing index shard {}", i);
            let meta_bin_size = bincode::serialized_size(&meta)?;
            indexes.push(Arc::new(RwLock::new(Index::with_vectorizer(i, vectorizer, meta, vectorizer_bin_size, meta_bin_size))));
        }
        info!("=Done loading all indexes=\n");
        Ok(Self { corpus, indexes, index_dir: path.to_string(), counter: AtomicU64::new(counter) })
    }

}