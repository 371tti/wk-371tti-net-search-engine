use reqwest::Client;

use crate::collect::ScraperResult;
// ScraperResult を直接返す
pub async fn fetch_scraper_api(
    api_url: &str,
) -> Result<ScraperResult, Box<dyn std::error::Error>> {
    let client = Client::new();
    let resp = client.get(api_url).send().await?.json::<ScraperResult>().await?;
    Ok(resp)
}