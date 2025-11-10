use kurosabi::utils::status::status_code::OK;



fn main() {
    
}

type Link = String;
type Links = Vec<Link>;

pub async fn worker(url: &str) -> Result<Links, Box<dyn std::error::Error>> {
    // ADD API にlinkを投げて帰ってきたLinksを収集する
    Ok(Links::new())
}