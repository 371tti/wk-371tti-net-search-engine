use std::{ops::Range, sync::{Arc, TryLockError}};

use futures::{StreamExt, stream::FuturesUnordered};
use log::{error, warn};
use tf_idf_vectorizer::{SimilarityAlgorithm, TokenFrequency};
use tokio::task::spawn_blocking;

use crate::{collect::{ResEntry, ScoredEntry}, engine::{IndexPool, tag::Tags}};

impl IndexPool {
    
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
    pub async fn per_similarity(&self, token_fq: &TokenFrequency, algorithm: &SimilarityAlgorithm) -> Vec<ScoredEntry> {
        let mut tasks = self.indexes.iter()
            .map(|arc_lock| { Arc::clone(arc_lock) })
            .map(|locked_idx| {
                let token_fq = token_fq.clone();
                let algorithm = algorithm.clone();
                async move {
                    spawn_blocking(move || {
                        let idx = match locked_idx.try_read() {
                            Ok(i) => i,
                            Err(e) => match e {
                                TryLockError::WouldBlock => {
                                    warn!("RwLock would block, skipping");
                                    return Vec::new();
                                }
                                TryLockError::Poisoned(e) => {
                                    error!("RwLock poisoned: {}", e);
                                    return Vec::new();
                                }
                            }
                        };
                        let id = idx.id;
                        idx.vectorizer
                            .similarity_uncheck_idf(&token_fq, &algorithm)
                            .list
                            .into_iter()
                            .map(|hit| ScoredEntry {
                                key: hit.0,
                                score: hit.1,
                                length: hit.2,
                                index_id: id,
                            })
                            .collect::<Vec<ScoredEntry>>()
                    }).await.unwrap_or_else(|_| Vec::new())
                }
            }).collect::<FuturesUnordered<_>>();

        let mut results = Vec::new();
        while let Some(mut shard_res) = tasks.next().await {
            results.append(&mut shard_res);
        }

        results
    }

    pub fn sort_by_score(&self, mut results: Vec<ScoredEntry>) -> Vec<ScoredEntry> {
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
    pub async fn generate_results(&self, results: Vec<ScoredEntry>, range: Range<usize>, tag: Tags, tag_exclusive: bool) -> Vec<ResEntry> {
        let mut res_entries = Vec::new();
        let mut counter = 0usize;
        for scored in results.iter() {
            let index = match self.indexes.get(scored.index_id) {
                Some(idx) => idx,
                None => continue,
            };

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

            let meta_opt = guard.meta_from_id(scored.key);
            let meta = match meta_opt {
                Some(m) => m.clone(),
                None => continue,
            };
            drop(guard);

            let tags = meta.tags;

            // タグフィルタリング
            if !tag.is_empty() {
                if tag_exclusive {
                    if !tags.is_filter_contains(tag) {
                        continue;
                    }
                } else {
                    if !tags.contains(tag) {
                        continue;
                    }
                }
            }

            counter += 1;
            if counter <= range.start {
                continue;
            }

            res_entries.push(ResEntry {
                url: meta.url,
                title: meta.title,
                favicon: meta.favicon,
                tags: tags.tags(),
                descriptions: meta.description,
                score: scored.score,
                point: meta.points,
                length: scored.length,
                id: scored.key,
                index_id: scored.index_id,
                time: meta.time,
            });
            if res_entries.len() >= range.len() {
                break;
            }
        }
        res_entries
    }
}