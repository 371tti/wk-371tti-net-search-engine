use reqwest::Client;
use serde_json::Value;

// descriptions, url, title(先頭), favicon(先頭)
pub async fn fetch_description_and_url(
    api_url: &str,
) -> Result<(Vec<String>, String, Option<String>, Option<String>), Box<dyn std::error::Error>> {
    let client = Client::new();
    let resp = client.get(api_url).send().await?.json::<Value>().await?;

    let url = resp.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let results = resp.get("results").cloned().unwrap_or(Value::Null);

    let descriptions = results
        .get("descriptions")
        .and_then(|d| d.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_else(Vec::new);

    let title = results
        .get("title")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let favicon = results
        .get("favicon")
        .and_then(|f| f.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok((descriptions, url, title, favicon))
}