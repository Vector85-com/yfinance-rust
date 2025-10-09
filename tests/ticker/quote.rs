use httpmock::Method::GET;
use httpmock::MockServer;
use serde_json::Value;
use url::Url;
use yfinance_rs::core::conversions::*;
use yfinance_rs::quote::QuotesBuilder;
use yfinance_rs::{Ticker, YfClient};

#[tokio::test]
async fn quote_v7_happy_path() {
    let server = MockServer::start();

    let body = r#"{
      "quoteResponse": {
        "result": [
          {
            "symbol":"AAPL",
            "regularMarketPrice": 190.25,
            "regularMarketPreviousClose": 189.50,
            "currency": "USD",
            "fullExchangeName": "NasdaqGS",
            "marketState": "REGULAR"
          }
        ],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, "AAPL");

    let q = ticker.quote().await.unwrap();
    mock.assert();

    assert_eq!(q.symbol, "AAPL");
    assert_eq!(
        q.exchange.as_ref().map(std::string::ToString::to_string),
        Some("NASDAQ".to_string())
    );
    assert_eq!(
        q.market_state
            .as_ref()
            .map(std::string::ToString::to_string),
        Some("REGULAR".to_string())
    );
    assert!((money_to_f64(&q.price.unwrap()) - 190.25).abs() < 1e-9);
    assert!((money_to_f64(&q.previous_close.unwrap()) - 189.50).abs() < 1e-9);
}

#[tokio::test]
async fn quote_raw_with_custom_fields() {
    let server = MockServer::start();

    let body = r#"{
      "quoteResponse": {
        "result": [
          {
            "symbol": "AAPL",
            "regularMarketChange": 1.23,
            "beta": 0.87
          }
        ],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL")
            .query_param("fields", "regularMarketChange,beta");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, "AAPL");

    let raw = ticker
        .quote_raw(&["regularMarketChange", "beta"])
        .await
        .unwrap();
    mock.assert();

    assert!(raw.is_object());
    assert_eq!(raw.get("symbol").and_then(Value::as_str), Some("AAPL"));
    assert!(
        (raw.get("regularMarketChange")
            .and_then(Value::as_f64)
            .unwrap()
            - 1.23)
            .abs()
            < 1e-9
    );
    assert!((raw.get("beta").and_then(Value::as_f64).unwrap() - 0.87).abs() < 1e-9);
    assert!(raw.get("regularMarketPrice").is_none());
}

#[tokio::test]
async fn quotes_builder_fetch_raw_with_fields() {
    let server = MockServer::start();

    let body = r#"{
      "quoteResponse": {
        "result": [
          { "symbol": "AAPL", "regularMarketChange": 1.0 },
          { "symbol": "TSLA", "regularMarketChange": -2.0 }
        ],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL,TSLA")
            .query_param("fields", "regularMarketChange");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let raw = QuotesBuilder::new(client)
        .symbols(["AAPL", "TSLA"])
        .fields(["regularMarketChange"])
        .fetch_raw()
        .await
        .unwrap();
    mock.assert();

    assert_eq!(raw.len(), 2);
    assert_eq!(raw[0].get("symbol").and_then(Value::as_str), Some("AAPL"));
    assert!(
        (raw[0]
            .get("regularMarketChange")
            .and_then(Value::as_f64)
            .unwrap()
            - 1.0)
            .abs()
            < 1e-9
    );
    assert_eq!(raw[1].get("symbol").and_then(Value::as_str), Some("TSLA"));
    assert!(
        (raw[1]
            .get("regularMarketChange")
            .and_then(Value::as_f64)
            .unwrap()
            + 2.0)
            .abs()
            < 1e-9
    );
}

#[tokio::test]
async fn fast_info_derives_last_price() {
    let server = MockServer::start();

    // Deliberately omit regularMarketPrice to test fallback → previous close
    let body = r#"{
      "quoteResponse": {
        "result": [
          {
            "symbol":"MSFT",
            "regularMarketPreviousClose": 421.00,
            "currency": "USD",
            "exchange": "NasdaqGS",
            "marketState": "CLOSED"
          }
        ],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, "MSFT");

    let fi = ticker.fast_info().await.unwrap();
    mock.assert();

    assert_eq!(fi.symbol, "MSFT");
    assert!(
        (fi.last_price - 421.00).abs() < 1e-9,
        "fallback to previous close"
    );
    assert_eq!(fi.currency.as_deref(), Some("USD"));
    assert_eq!(fi.exchange.as_deref(), Some("NASDAQ"));
    assert_eq!(fi.market_state.as_deref(), Some("CLOSED"));
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_quote_smoke() {
    if std::env::var("YF_LIVE").ok().as_deref() != Some("1")
        && std::env::var("YF_RECORD").ok().as_deref() != Some("1")
    {
        return;
    }

    let client = YfClient::builder().build().unwrap();
    let ticker = Ticker::new(&client, "AAPL");
    let fi = ticker.fast_info().await.unwrap();

    if std::env::var("YF_RECORD").ok().as_deref() != Some("1") {
        assert!(fi.last_price > 0.0);
        assert_eq!(fi.symbol, "AAPL");
    }
}
