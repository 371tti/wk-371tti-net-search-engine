use std::{io::Error, path::Path};

use log::error;

use crate::engine::IndexPool;

impl IndexPool {
    /// Save indexes and corpus to the specified directory
    pub async fn save(&self, path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        std::fs::create_dir_all(path)?;

        // Save corpus
        let corpus_path = Path::new(path).join("global.corpus");
        let corpus_data = bincode::serialize(&*self.corpus)?;
        std::fs::write(corpus_path, corpus_data)?;

        // Save each index and meta
        for entry in self.indexes.iter() {
            let index = match entry.read() {
                Ok(i) => i,
                Err(e) => {
                    error!("RwLock poisoned: {}", e);
                    continue;
                },
            };
            let index_path = Path::new(path).join(format!("{}.index", index.id));
            let meta_path = Path::new(path).join(format!("{}.meta", index.id));

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
    pub async fn save_shard(&self, shard_id: usize, path: &str) -> Result<(u64, u64), Box<dyn std::error::Error + Send + Sync>> {
        std::fs::create_dir_all(path)?;

        // Save corpus
        let corpus_path = Path::new(path).join("global.corpus");
        let corpus_file = std::fs::File::create(&corpus_path)?;
        let mut corpus_writer = std::io::BufWriter::new(corpus_file);
        bincode::serialize_into(&mut corpus_writer, &*self.corpus)?;

        // Save specified index and meta
        if let Some(entry) = self.indexes.get(shard_id) {
            let index = match entry.read() {
                Ok(i) => i,
                Err(e) => {
                    error!("RwLock poisoned: {}", e);
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, "RwLock poisoned").into());
                },
            };

            // Save vectorizer
            let index_path = Path::new(path).join(format!("{}.index", index.id));
            let index_file = std::fs::File::create(&index_path)?;
            let mut index_writer = std::io::BufWriter::new(index_file);
            bincode::serialize_into(&mut index_writer, &index.vectorizer)?;

            // Save metadata
            let meta_path = Path::new(path).join(format!("{}.meta", index.id));
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
}