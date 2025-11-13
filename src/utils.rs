use tf_idf_vectorizer::SimilarityAlgorithm;

use crate::{DEFAULT_SEARCH_RESULTS, MAX_SEARCH_RESULTS};

// 検索アルゴリズムの簡易パーサ
pub fn parse_algo(s: &str) -> SimilarityAlgorithm {
    let lower = s.trim().to_ascii_lowercase();
    // 補助: 引数の括弧内から数値を抽出
    fn nums(src: &str) -> Vec<f64> {
        if let (Some(l), Some(r)) = (src.find('('), src.rfind(')')) {
            let inner = &src[l + 1..r];
            inner
                .split(',')
                .filter_map(|p| p.trim().parse::<f64>().ok())
                .collect()
        } else {
            Vec::new()
        }
    }

    if lower.starts_with("dot") {
        SimilarityAlgorithm::Dot
    } else if lower.starts_with("cosine") || lower.starts_with("cosinesimilarity") {
        SimilarityAlgorithm::CosineSimilarity
    } else if lower.starts_with("bm25") {
        // BM25(k1, b) の形式を受け取る。引数が省略された場合は既定値を使う。
        let vals = nums(&lower);
        let k1 = vals.get(0).copied().unwrap_or(1.2);
        let b = vals.get(1).copied().unwrap_or(0.75);
        SimilarityAlgorithm::BM25(k1, b)
    } else {
        // 既定
        SimilarityAlgorithm::BM25(1.2, 0.75)
    }
}

// range クエリ文字列を正規化して (start, end) (endは排他的) を返す
// 受け入れる形式:
//   "a..b"  -> a..b
//   "..b"   -> 0..b
//   "a.."   -> a..a+DEFAULT_SEARCH_RESULTS
//   "v"     -> v..v+DEFAULT_SEARCH_RESULTS
//   空/None  -> 0..DEFAULT_SEARCH_RESULTS
// 正規化:
//   1) 解析失敗はデフォルト
//   2) end < start の場合 swap (例: 20..10 -> 10..20)
//   3) 幅 > MAX_SEARCH_RESULTS の場合 end = start + MAX_SEARCH_RESULTS
//   4) 加算は saturating_add でオーバーフロー防止
pub fn parse_range_param(raw: Option<String>) -> (usize, usize) {
    let default_end = DEFAULT_SEARCH_RESULTS.min(MAX_SEARCH_RESULTS);
    let Some(s) = raw else { return (0, default_end); };
    if s.is_empty() { return (0, default_end); }

    let (mut start, mut end) = if let Some((l, r)) = s.split_once("..") {
        // a..b / a.. / ..b
        let start = if l.is_empty() { 0 } else { l.parse::<usize>().unwrap_or(0) };
        if r.is_empty() {
            // a..  -> a..a+DEFAULT
            let tentative = start.saturating_add(DEFAULT_SEARCH_RESULTS);
            (start, tentative)
        } else {
            // a..b / ..b
            let end = r.parse::<usize>().unwrap_or(start);
            let start = if l.is_empty() { 0 } else { start }; // ..b の場合 start=0
            (start, end)
        }
    } else {
        // 単値 v
        let v = s.parse::<usize>().unwrap_or(0);
        let end = v.saturating_add(DEFAULT_SEARCH_RESULTS);
        (v, end)
    };

    // swap if reversed
    if end < start { std::mem::swap(&mut start, &mut end); }

    // 幅制限
    let max_end = start.saturating_add(MAX_SEARCH_RESULTS);
    if end > max_end { end = max_end; }

    (start, end)
}


// シンプルな URL クエリ値用パーセントエンコーダ（RFC3986 の unreserved を除いて byte を %HH に変換）
pub fn percent_encode_query_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}
