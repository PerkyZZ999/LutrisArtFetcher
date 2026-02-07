/// `SteamGridDB` API v2 client.
///
/// Thin async wrapper around `reqwest` for searching games, fetching asset lists,
/// and downloading images. Includes configurable request delay to respect rate limits.
use std::time::Duration;

use color_eyre::eyre::{Context, Result, eyre};
use reqwest::Client;

use super::models::{ApiResponse, AssetType, ImageAsset, SearchResult};

const BASE_URL: &str = "https://www.steamgriddb.com/api/v2";

/// Async client for the `SteamGridDB` REST API.
pub struct SteamGridDbClient {
    client: Client,
    request_delay: Duration,
}

impl SteamGridDbClient {
    /// Create a new client with the given API key and inter-request delay.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(api_key: &str, delay_ms: u64) -> Result<Self> {
        let client = Client::builder()
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                let val = reqwest::header::HeaderValue::from_str(&format!("Bearer {api_key}"))
                    .wrap_err("Invalid API key format")?;
                headers.insert(reqwest::header::AUTHORIZATION, val);
                headers
            })
            .timeout(Duration::from_secs(30))
            .build()
            .wrap_err("Failed to build HTTP client")?;

        Ok(Self {
            client,
            request_delay: Duration::from_millis(delay_ms),
        })
    }

    /// Validate the API key by hitting a known endpoint.
    ///
    /// Returns `true` if the server responds with 200.
    pub async fn validate_key(&self) -> Result<bool> {
        let url = format!("{BASE_URL}/grids/game/1?dimensions=600x900");
        let resp = self.client.get(&url).send().await.wrap_err("Key validation request failed")?;
        Ok(resp.status().is_success())
    }

    /// Search for a game by name. Slugs should be pre-converted (replace `-` with space).
    pub async fn search(&self, term: &str) -> Result<Vec<SearchResult>> {
        let url = format!("{BASE_URL}/search/autocomplete/{term}");
        self.delay().await;

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .wrap_err_with(|| format!("Search request failed for '{term}'"))?;

        if !resp.status().is_success() {
            return Err(eyre!("Search failed with status {}", resp.status()));
        }

        let body: ApiResponse<SearchResult> = resp
            .json()
            .await
            .wrap_err("Failed to parse search response")?;

        Ok(body.data)
    }

    /// Fetch asset images for a game by its `SteamGridDB` ID.
    pub async fn get_assets(
        &self,
        asset_type: AssetType,
        game_id: u64,
        dimensions: Option<&str>,
    ) -> Result<Vec<ImageAsset>> {
        let mut url = format!("{BASE_URL}/{}/game/{game_id}", asset_type.api_path());
        if let Some(dims) = dimensions {
            use std::fmt::Write;
            let _ = write!(url, "?dimensions={dims}");
        }
        self.delay().await;

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .wrap_err_with(|| format!("Asset request failed for game {game_id}"))?;

        if !resp.status().is_success() {
            return Err(eyre!(
                "Asset fetch failed with status {} for game {game_id}",
                resp.status()
            ));
        }

        let body: ApiResponse<ImageAsset> = resp
            .json()
            .await
            .wrap_err("Failed to parse asset response")?;

        Ok(body.data)
    }

    /// Fetch assets using a platform-specific ID (e.g. Steam app ID) for a more
    /// accurate match than text search.
    pub async fn get_assets_by_platform(
        &self,
        asset_type: AssetType,
        platform: &str,
        platform_id: &str,
        dimensions: Option<&str>,
    ) -> Result<Vec<ImageAsset>> {
        let mut url = format!(
            "{BASE_URL}/{}/{platform}/{platform_id}",
            asset_type.api_path()
        );
        if let Some(dims) = dimensions {
            use std::fmt::Write;
            let _ = write!(url, "?dimensions={dims}");
        }
        self.delay().await;

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .wrap_err_with(|| {
                format!("Platform asset request failed for {platform}/{platform_id}")
            })?;

        if !resp.status().is_success() {
            // Platform lookup can 404 for non-Steam games; not an error per se
            return Ok(Vec::new());
        }

        let body: ApiResponse<ImageAsset> = resp
            .json()
            .await
            .wrap_err("Failed to parse platform asset response")?;

        Ok(body.data)
    }

    /// Download raw image bytes from a CDN URL.
    pub async fn download_image(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .wrap_err_with(|| format!("Image download failed for {url}"))?;

        if !resp.status().is_success() {
            return Err(eyre!("Image download returned status {}", resp.status()));
        }

        let bytes = resp
            .bytes()
            .await
            .wrap_err("Failed to read image bytes")?;
        Ok(bytes.to_vec())
    }

    /// Sleep for the configured inter-request delay.
    async fn delay(&self) {
        if !self.request_delay.is_zero() {
            tokio::time::sleep(self.request_delay).await;
        }
    }
}
