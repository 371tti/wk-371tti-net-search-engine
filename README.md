# wk-371tti-net-search-engine

Rust 製のシンプルな TF-IDF / BM25 系検索エンジン API サーバ。
外部スクレイパ API からメタ情報を取得しインデックス化、HTTP 経由で検索を提供します。

## 特徴
- TF-IDF / BM25 / 派生アルゴリズム対応 (`SimilarityAlgorithm`)
- ドキュメント追加 `/add` と検索 `/search` の最小 API
- range=n..m 形式で結果ページング
- タグフィルタ (OR / AND: `tag_exclusive=true`)
- Sudachi 形態素解析による日本語トークナイズ (A モード)
- Ctrl+C 時にインデックス保存 (予定/実装中部分はコード参照)
- `env_logger` によるログ出力 (RUST_LOG で制御)

## ビルド & 実行
```bash
cargo build --release
cargo run --release
```
Windows PowerShell 例:
```pwsh
cargo run --release
```

## 環境変数
| 変数 | 説明 | 例 |
|------|------|----|
| RUST_LOG | ログレベル | `info`, `debug`, `trace` |

未設定なら `info` がデフォルト。詳細デバッグ時は `RUST_LOG=debug` 推奨。

## エンドポイント
### 1. ドキュメント追加 `POST /add`
Request JSON (例):
```json
{
  "url": "https://example.com/",
  "title": "任意タイトル(省略可)",
  "favicon": "https://example.com/favicon.ico",
  "tags": ["wiki", "news"],
  "descriptions": "任意の説明文 (省略可)"
}
```
サーバ側でスクレイパ API (SCRAPER_API_URL) を呼び、タイトル/description 不足分を補完。

Response (成功):
```json
{
  "success": true,
  "url": "https://example.com/",
  "title": "Example Domain",
  "favicon": null,
  "tags": ["WIKI"],
  "descriptions": "Example Domain Example ..."
}
```

### 2. 検索 `GET /search`
クエリパラメータ:
| パラメータ | 説明 | 例 |
|------------|------|----|
| query | 検索クエリ (必須) | `rust tfidf` |
| range | 返却範囲 a..b (bは排他的) | `0..20`, `20..40`, `..50`, `30..` |
| algo | アルゴリズム | `BM25(1.2,0.75)` / `BM25plus()` / `Cosine` |
| tag | カンマ区切りタグ | `wiki,news` |
| tag_exclusive | AND 条件にする | `true` / `1` |

タグは以下 (OR / AND 指定可能): `wiki, news, sns, blog, forum, shopping, academic, tools`

Response (成功スニペット):
```json
{
  "success": true,
  "query": "rust",
  "algorithm": "BM25(1.2,0.75)",
  "range": {"start":0, "end":20},
  "results": [
    {
      "url": "https://example.com/",
      "title": "Example Domain",
      "score": 3.42,
      "length": 1200,
      "favicon": null,
      "id": 0,
      "index_id": 0,
      "tags": ["WIKI"],
      "descriptions": "Example Domain ..."
    }
  ]
}
```

### 3. ステータス `GET /status`
インデックス済み件数など。

## range 仕様
- `a..b` 明示範囲
- `..b` は `0..b`
- `a..` は `a..a+DEFAULT_SEARCH_RESULTS`
- 単値 `v` は `v..v+DEFAULT_SEARCH_RESULTS`
- 最大幅 `MAX_SEARCH_RESULTS`

