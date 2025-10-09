use std::path::PathBuf;
use std::sync::Arc;

use sudachi::analysis::stateless_tokenizer::StatelessTokenizer;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::prelude::*;
use sudachi::analysis::Tokenize;

/// std io 経由で呼び出すのが遅いので、Sudachiのライブラリを直接使う版
pub struct SudachiTokenizer {
    dictionary: Arc<JapaneseDictionary>,
}

impl SudachiTokenizer {
    const SUDACHI_CONFIG: &str = "./config/sudachi.json";

    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = Config::new(Some(PathBuf::from(Self::SUDACHI_CONFIG)), None, None)?;
        let dict = Arc::new(JapaneseDictionary::from_cfg(&config)?);
        Ok(Self { dictionary: dict })
    }

    fn new_tokenizer(&self) -> StatelessTokenizer<Arc<JapaneseDictionary>> {
        StatelessTokenizer::new(Arc::clone(&self.dictionary))
    }

    pub fn tokenize(&self, text: &str, mode: Mode) -> Result<Tokenized, Box<dyn std::error::Error>> {
        let tokenizer = self.new_tokenizer();
        let result = tokenizer.tokenize(text, mode, false)?;
        Ok(Tokenized {
            result,
        })
    }

    pub fn pure_doc_tokenizer(&self, text: &str) -> Result<Vec<Box<str>>, Box<dyn std::error::Error>> {
        let c = self.tokenize(text, Mode::C)?;
        Ok(c.tokens())
    }

    pub fn mix_doc_tokenizer(&self, text: &str) -> Result<Vec<Box<str>>, Box<dyn std::error::Error>> {
        let c = self.tokenize(text, Mode::C)?;
        let a = self.tokenize(text, Mode::A)?;
        let normalized_c_tokens = c.normalized_tokens();
        let a_tokens = a.tokens();
        let a_2gram_tokens: Vec<Box<str>> = a_tokens.windows(2)
            .map(|w| format!("{}{}", w[0], w[1]).into_boxed_str())
            .collect();
        let a_speech_tokens = a.speech_tokens();
        let synthetic_tokens: Vec<Box<str>> = normalized_c_tokens.into_iter()
            .chain(a_2gram_tokens.into_iter())
            .chain(a_speech_tokens.into_iter())
            .collect();
        Ok(synthetic_tokens)
    }

    pub fn pure_query_tokenizer(&self, text: &str) -> Result<Vec<Box<str>>, Box<dyn std::error::Error>> {
        let c = self.tokenize(text, Mode::C)?;
        Ok(c.normalized_tokens())
    }

    pub fn mix_query_tokenizer(&self, text: &str) -> Result<Vec<Box<str>>, Box<dyn std::error::Error>> {
        let c = self.tokenize(text, Mode::C)?;
        let a = self.tokenize(text, Mode::A)?;
        let normalized_c_tokens = c.normalized_tokens();
        let a_tokens = a.tokens();
        let a_2gram_tokens: Vec<Box<str>> = a_tokens.windows(2)
            .map(|w| format!("{}{}", w[0], w[1]).into_boxed_str())
            .collect();
        let a_speech_tokens = a.speech_tokens();
        let synthetic_tokens: Vec<Box<str>> = normalized_c_tokens.into_iter()
            .chain(a_2gram_tokens.into_iter())
            .chain(a_speech_tokens.into_iter())
            .collect();
        Ok(synthetic_tokens)
    }
}

pub struct Tokenized {
    result: MorphemeList<Arc<JapaneseDictionary>>,
}

impl Tokenized {
    pub fn normalized_tokens(&self) -> Vec<Box<str>> {
        self.result
            .iter()
            .map(|s| s.normalized_form().trim_matches(&[' ', '　']).to_string().into_boxed_str())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn tokens(&self) -> Vec<Box<str>> {
        self.result
            .iter()
            .map(|m| m.surface().trim_matches(&[' ', '　']).to_string().into_boxed_str())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn speech_tokens(&self) -> Vec<Box<str>> {
        self.result
            .iter()
            .map(|m| m.reading_form().replace("キゴウ", "").trim().to_string().into_boxed_str())
            .filter(|s| !s.is_empty())
            .collect()
    }
}