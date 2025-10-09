use crate::core::{
    YfClient, YfError,
    client::{CacheMode, RetryConfig},
    models::Quote,
    quotes,
};
use serde_json::Value;

pub async fn fetch_quote(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Quote, YfError> {
    let symbols = [symbol];
    let mut results =
        quotes::fetch_v7_quotes(client, &symbols, None, cache_mode, retry_override).await?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    // Use the same currency-aware conversion as the batch quotes API
    Ok(result.into())
}

pub async fn fetch_quote_raw(
    client: &YfClient,
    symbol: &str,
    fields: Option<&[&str]>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Value, YfError> {
    let symbols = [symbol];
    let mut results =
        quotes::fetch_v7_quotes_raw(client, &symbols, fields, cache_mode, retry_override).await?;

    results
        .pop()
        .ok_or_else(|| YfError::MissingData(format!("no quote result found for symbol {symbol}")))
}
