/// `Steam` Store API client used as a secondary asset source.
///
/// This client uses public store endpoints (search + appdetails) and Steam CDN
/// URL patterns to find downloadable art by app ID.
use std::collections::HashSet;
use std::time::Duration;

use color_eyre::eyre::{eyre, Context, Result};
use reqwest::Client;
use serde::Deserialize;

use super::models::AssetType;

const STORE_BASE_URL: &str = "https://store.steampowered.com/api";
const CDN_BASE_URL: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps";

/// Search candidate returned by the Steam store search endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct SteamAppCandidate {
    pub id: u32,
    pub name: String,
    #[serde(default, rename = "type")]
    pub app_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct StoreSearchResponse {
    #[serde(default)]
    items: Vec<SteamAppCandidate>,
}

#[derive(Debug, Clone, Deserialize)]
struct AppDetailsEnvelope {
    success: bool,
    data: Option<AppDetailsData>,
}

#[derive(Debug, Clone, Deserialize)]
struct AppDetailsData {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    app_type: String,
    #[serde(default)]
    header_image: Option<String>,
    #[serde(default)]
    capsule_image: Option<String>,
}

/// Minimal Steam app details needed for fallback selection and URL generation.
#[derive(Debug, Clone)]
pub struct SteamAppDetails {
    pub app_id: u32,
    pub name: String,
    pub app_type: String,
    pub header_image: Option<String>,
    pub capsule_image: Option<String>,
}

impl SteamAppDetails {
    /// Construct details when appdetails lookup fails but we still trust the app ID.
    pub fn placeholder(app_id: u32) -> Self {
        Self {
            app_id,
            name: String::new(),
            app_type: String::new(),
            header_image: None,
            capsule_image: None,
        }
    }
}

/// Async Steam store helper client.
pub struct SteamStoreClient {
    client: Client,
    request_delay: Duration,
}

impl SteamStoreClient {
    /// Create a new client with request delay for rate-limit friendliness.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(delay_ms: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .wrap_err("failed to build Steam store HTTP client")?;

        Ok(Self {
            client,
            request_delay: Duration::from_millis(delay_ms),
        })
    }

    /// Search Steam apps by name.
    ///
    /// # Errors
    ///
    /// Returns an error for transport failures, HTTP failures, or invalid JSON.
    pub async fn search_apps(&self, term: &str) -> Result<Vec<SteamAppCandidate>> {
        self.delay().await;

        let resp = self
            .client
            .get(format!("{STORE_BASE_URL}/storesearch"))
            .query(&[("term", term), ("l", "english"), ("cc", "us")])
            .send()
            .await
            .wrap_err_with(|| format!("Steam store search request failed for '{term}'"))?;

        if !resp.status().is_success() {
            return Err(eyre!(
                "Steam store search failed with status {}",
                resp.status()
            ));
        }

        let body: StoreSearchResponse = resp
            .json()
            .await
            .wrap_err("failed to parse Steam store search response")?;

        Ok(body.items)
    }

    /// Fetch app details for one app ID.
    ///
    /// # Errors
    ///
    /// Returns an error for transport failures, HTTP failures, or invalid JSON.
    pub async fn get_app_details(&self, app_id: u32) -> Result<Option<SteamAppDetails>> {
        self.delay().await;

        let resp = self
            .client
            .get(format!("{STORE_BASE_URL}/appdetails"))
            .query(&[
                ("appids", app_id.to_string()),
                ("l", "english".to_owned()),
                ("cc", "us".to_owned()),
            ])
            .send()
            .await
            .wrap_err_with(|| format!("Steam appdetails request failed for appid {app_id}"))?;

        if !resp.status().is_success() {
            return Err(eyre!(
                "Steam appdetails failed with status {} for appid {app_id}",
                resp.status()
            ));
        }

        let body: std::collections::HashMap<String, AppDetailsEnvelope> = resp
            .json()
            .await
            .wrap_err("failed to parse Steam appdetails response")?;

        let key = app_id.to_string();
        let Some(entry) = body.get(&key) else {
            return Ok(None);
        };

        if !entry.success {
            return Ok(None);
        }

        let Some(data) = &entry.data else {
            return Ok(None);
        };

        Ok(Some(SteamAppDetails {
            app_id,
            name: data.name.clone(),
            app_type: data.app_type.clone(),
            header_image: data.header_image.clone(),
            capsule_image: data.capsule_image.clone(),
        }))
    }

    /// Build candidate Steam CDN URLs for an asset type.
    pub fn candidate_image_urls(
        &self,
        asset: AssetType,
        details: &SteamAppDetails,
        grid_dim: &str,
    ) -> Vec<String> {
        let app_id = details.app_id;
        let mut urls = Vec::new();

        match asset {
            AssetType::Grid => {
                urls.push(format!("{CDN_BASE_URL}/{app_id}/library_{grid_dim}.jpg"));
                if grid_dim != "600x900" {
                    urls.push(format!("{CDN_BASE_URL}/{app_id}/library_600x900.jpg"));
                }
                urls.push(format!("{CDN_BASE_URL}/{app_id}/library_600x900.jpg"));
                urls.push(format!("{CDN_BASE_URL}/{app_id}/header.jpg"));
                urls.push(format!("{CDN_BASE_URL}/{app_id}/capsule_616x353.jpg"));
                if let Some(url) = &details.capsule_image {
                    urls.push(url.clone());
                }
                if let Some(url) = &details.header_image {
                    urls.push(url.clone());
                }
            }
            AssetType::Hero => {
                urls.push(format!("{CDN_BASE_URL}/{app_id}/library_hero.jpg"));
                urls.push(format!("{CDN_BASE_URL}/{app_id}/header.jpg"));
                if let Some(url) = &details.header_image {
                    urls.push(url.clone());
                }
            }
            AssetType::Logo => {
                urls.push(format!("{CDN_BASE_URL}/{app_id}/logo.png"));
                urls.push(format!("{CDN_BASE_URL}/{app_id}/logo.jpg"));
            }
            AssetType::Icon => {
                // Steam does not provide a stable icon endpoint for all apps;
                // use logo as the fallback as requested.
                urls.push(format!("{CDN_BASE_URL}/{app_id}/logo.png"));
                urls.push(format!("{CDN_BASE_URL}/{app_id}/logo.jpg"));
                if let Some(url) = &details.capsule_image {
                    urls.push(url.clone());
                }
                if let Some(url) = &details.header_image {
                    urls.push(url.clone());
                }
            }
        }

        dedup_urls(urls)
    }

    /// Download image bytes from a Steam CDN URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or server returns a non-success status.
    pub async fn download_image(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .wrap_err_with(|| format!("Steam image download failed for {url}"))?;

        if !resp.status().is_success() {
            return Err(eyre!(
                "Steam image download returned status {}",
                resp.status()
            ));
        }

        let bytes = resp
            .bytes()
            .await
            .wrap_err("failed to read Steam image bytes")?;
        Ok(bytes.to_vec())
    }

    async fn delay(&self) {
        if !self.request_delay.is_zero() {
            tokio::time::sleep(self.request_delay).await;
        }
    }
}

fn dedup_urls(urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(urls.len());

    for url in urls {
        if seen.insert(url.clone()) {
            out.push(url);
        }
    }

    out
}
