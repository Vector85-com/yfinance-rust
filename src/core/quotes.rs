// src/core/quotes.rs
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::{
    YfClient, YfError,
    core::{
        client::{CacheMode, RetryConfig},
        conversions::f64_to_money_with_currency_str,
        net,
    },
};
use paft::market::quote::Quote;

// Centralized wire model for the v7 quote API
#[derive(Deserialize)]
pub struct V7Envelope {
    #[serde(rename = "quoteResponse")]
    pub(crate) quote_response: Option<V7QuoteResponse>,
}

#[derive(Deserialize)]
pub struct V7QuoteResponse {
    pub(crate) result: Option<Vec<V7QuoteNode>>,
    #[allow(dead_code)]
    pub(crate) error: Option<serde_json::Value>,
}

#[derive(Deserialize, Clone)]
pub struct V7QuoteNode {
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    #[serde(rename = "shortName")]
    pub(crate) short_name: Option<String>,
    #[serde(rename = "regularMarketPrice")]
    pub(crate) regular_market_price: Option<f64>,
    #[serde(rename = "regularMarketPreviousClose")]
    pub(crate) regular_market_previous_close: Option<f64>,
    pub(crate) currency: Option<String>,
    #[serde(rename = "fullExchangeName")]
    pub(crate) full_exchange_name: Option<String>,
    pub(crate) exchange: Option<String>,
    pub(crate) market: Option<String>,
    #[serde(rename = "marketCapFigureExchange")]
    pub(crate) market_cap_figure_exchange: Option<String>,
    #[serde(rename = "marketState")]
    pub(crate) market_state: Option<String>,
}

async fn fetch_v7_quote_body(
    client: &YfClient,
    symbols: &[&str],
    fields: Option<&[&str]>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<String, YfError> {
    // Inner function to attempt the fetch, allowing for an auth retry.
    async fn attempt_fetch(
        client: &YfClient,
        symbols: &[&str],
        fields: Option<&[&str]>,
        crumb: Option<&str>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<(String, Url, Option<u16>), YfError> {
        let mut url = client.base_quote_v7().clone();
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("symbols", &symbols.join(","));
            if let Some(list) = fields {
                if !list.is_empty() {
                    qp.append_pair("fields", &list.join(","));
                }
            }
            if let Some(c) = crumb {
                qp.append_pair("crumb", c);
            }
        }

        if cache_mode == CacheMode::Use
            && let Some(body) = client.cache_get(&url).await
        {
            return Ok((body, url, None));
        }

        let resp = client
            .send_with_retry(
                client
                    .http()
                    .get(url.clone())
                    .header("accept", "application/json"),
                retry_override,
            )
            .await?;

        let status = resp.status();
        let body = net::get_text(resp, "quote_v7", &symbols.join("-"), "json").await?;

        if status.is_success() {
            if cache_mode != CacheMode::Bypass {
                client.cache_put(&url, &body, None).await;
            }
            Ok((body, url, None))
        } else {
            Ok((body, url, Some(status.as_u16())))
        }
    }

    // First attempt, without a crumb.
    let (body, url, maybe_status) =
        attempt_fetch(client, symbols, fields, None, cache_mode, retry_override).await?;

    let body_to_parse = if let Some(status_code) = maybe_status {
        // If unauthorized, get a crumb and retry.
        if status_code == 401 || status_code == 403 {
            client.ensure_credentials().await?;
            let crumb = client.crumb().await.ok_or_else(|| {
                YfError::Auth("Crumb is not set after ensuring credentials".into())
            })?;

            // Second attempt, with a crumb.
            let (body, url, maybe_status) = attempt_fetch(
                client,
                symbols,
                fields,
                Some(&crumb),
                cache_mode,
                retry_override,
            )
            .await?;

            if let Some(status_code) = maybe_status {
                let url_s = url.to_string();
                return Err(match status_code {
                    404 => YfError::NotFound { url: url_s },
                    429 => YfError::RateLimited { url: url_s },
                    500..=599 => YfError::ServerError {
                        status: status_code,
                        url: url_s,
                    },
                    _ => YfError::Status {
                        status: status_code,
                        url: url_s,
                    },
                });
            }
            body
        } else {
            let url_s = url.to_string();
            return Err(match status_code {
                404 => YfError::NotFound { url: url_s },
                429 => YfError::RateLimited { url: url_s },
                500..=599 => YfError::ServerError {
                    status: status_code,
                    url: url_s,
                },
                _ => YfError::Status {
                    status: status_code,
                    url: url_s,
                },
            });
        }
    } else {
        body
    };

    Ok(body_to_parse)
}

/// Centralized function to fetch one or more quotes from the v7 API.
/// It handles caching, retries, and authentication (crumb).
pub async fn fetch_v7_quotes(
    client: &YfClient,
    symbols: &[&str],
    fields: Option<&[&str]>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<V7QuoteNode>, YfError> {
    let body = fetch_v7_quote_body(client, symbols, fields, cache_mode, retry_override).await?;
    let env: V7Envelope = serde_json::from_str(&body)?;

    Ok(env
        .quote_response
        .and_then(|qr| qr.result)
        .unwrap_or_default())
}

/// Fetches raw quote nodes from the v7 API without mapping to strongly typed models.
pub async fn fetch_v7_quotes_raw(
    client: &YfClient,
    symbols: &[&str],
    fields: Option<&[&str]>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<Value>, YfError> {
    let body = fetch_v7_quote_body(client, symbols, fields, cache_mode, retry_override).await?;
    let value: Value = serde_json::from_str(&body)?;

    let nodes = value
        .get("quoteResponse")
        .and_then(|qr| qr.get("result"))
        .and_then(|res| res.as_array())
        .map(|arr| arr.clone())
        .unwrap_or_default();

    Ok(nodes)
}

impl From<V7QuoteNode> for Quote {
    fn from(n: V7QuoteNode) -> Self {
        Self {
            symbol: n.symbol.unwrap_or_default(),
            shortname: n.short_name,
            price: n
                .regular_market_price
                .map(|price| f64_to_money_with_currency_str(price, n.currency.as_deref())),
            previous_close: n
                .regular_market_previous_close
                .map(|price| f64_to_money_with_currency_str(price, n.currency.as_deref())),
            exchange: crate::core::conversions::string_to_exchange(
                n.full_exchange_name
                    .or(n.exchange)
                    .or(n.market)
                    .or(n.market_cap_figure_exchange),
            ),
            market_state: n.market_state.and_then(|s| s.parse().ok()),
        }
    }
}
