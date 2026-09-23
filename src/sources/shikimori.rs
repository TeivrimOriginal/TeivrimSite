use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, serde::Serialize, Deserialize)]
pub struct ShikiAnime {
    pub id: i64,
    pub name: Option<String>,
    pub russian: Option<String>,
    pub english: Option<Vec<String>>,
    pub japanese: Option<Vec<String>>,
    pub synonyms: Option<Vec<String>>,
    pub kind: Option<String>,
    pub rating: Option<String>,
    #[serde(default, deserialize_with = "de_score")]
    pub score: Option<f64>,
    pub status: Option<String>,
    pub episodes: Option<i64>,
    pub episodes_aired: Option<i64>,
    pub aired_on: Option<String>,
    pub released_on: Option<String>,
    pub description: Option<String>,
    pub image: Option<serde_json::Value>,
    pub studios: Option<Vec<Named>>,
    pub genres: Option<Vec<Named>>,
    pub screenshots: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, serde::Serialize, Deserialize)]
pub struct Named {
    pub name: Option<String>,
    pub russian: Option<String>,
}

/// Shikimori отдаёт score как строку "8.4" или null.
fn de_score<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let opt: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match opt {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => {
            if s.is_empty() {
                Ok(None)
            } else {
                Ok(s.parse::<f64>().ok())
            }
        }
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64()),
        _ => Ok(None),
    }
}

pub async fn fetch_page(
    client: &reqwest::Client,
    page: u32,
) -> Result<(Vec<ShikiAnime>, bool), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!(
        "https://shikimori.one/api/animes?page={}&limit=50&order=popularity",
        page
    );

    let resp = client.get(&url).send().await?;

    let status = resp.status();
    if status.as_u16() == 429 {
        eprintln!("[Shikimori RATE LIMIT] 429. Ждём 5 сек...");
        tokio::time::sleep(Duration::from_secs(5)).await;
        return Err("429".into());
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    // Читаем тело как строку и парсим поштучно,
    // чтобы одно битое аниме не ломало всю страницу.
    let text = resp.text().await?;
    let items_raw: Vec<serde_json::Value> = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Err(format!("JSON parse error: {}", e).into()),
    };

    let mut items: Vec<ShikiAnime> = Vec::new();
    for raw in items_raw {
        match serde_json::from_value::<ShikiAnime>(raw.clone()) {
            Ok(a) => items.push(a),
            Err(e) => {
                let id = raw.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
                eprintln!("[Shikimori] пропуск id={} ({})", id, e);
            }
        }
    }

    let has_next = items.len() >= 40;

    Ok((items, has_next))
}

/// Проверка одного аниме перед полной загрузкой.
pub async fn ping_test(
    client: &reqwest::Client,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let url = "https://shikimori.one/api/animes/16498";

    let resp = client.get(url).send().await?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status).into());
    }

    let v: serde_json::Value = resp.json().await?;
    let russian = v
        .get("russian")
        .and_then(|x| x.as_str())
        .map(String::from);

    Ok(russian)
}