use crate::config::DownloadConfig;
use crate::error::{DownloadError, DownloadResult};
use bytes::Bytes;
use reqwest::Client;
use reqwest::header::{self, HeaderMap, HeaderValue, RANGE};
/// 构建 HTTP 客户端。
pub fn build_client(config: &DownloadConfig) -> DownloadResult<Client> {
    let mut builder = Client::builder()
        .timeout(config.timeout)
        .danger_accept_invalid_certs(false);

    // 代理
    if let Some(ref proxy_url) = config.proxy.url {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| DownloadError::InvalidUrl(format!("代理 URL 无效: {e}")))?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(DownloadError::Network)
}

/// 构建公共请求头。
pub fn build_headers(config: &DownloadConfig) -> HeaderMap {
    let mut headers = HeaderMap::new();

    for (key, value) in &config.headers {
        if let (Ok(k), Ok(v)) = (
            header::HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.insert(k, v);
        }
    }

    // Cookie
    if let Some(ref cookie) = config.cookie {
        if let Ok(v) = HeaderValue::from_str(cookie) {
            headers.insert(header::COOKIE, v);
        }
    }

    headers
}

/// 探测文件信息（大小 + Range 支持）。
pub async fn probe(
    client: &Client,
    url: &str,
    config: &DownloadConfig,
) -> DownloadResult<(Option<u64>, bool)> {
    let mut req = client.head(url);
    let headers = build_headers(config);

    // 设置认证
    if let Some(ref auth) = config.auth.basic {
        req = req.basic_auth(&auth.0, Some(&auth.1));
    }
    if let Some(ref token) = config.auth.bearer {
        req = req.bearer_auth(token);
    }

    let resp = req.headers(headers).send().await?;

    let status = resp.status();
    if !status.is_success() && status.as_u16() != 200 {
        // 有些服务器对 HEAD 返回 405，此时降级处理
        if status.as_u16() == 405 {
            return Ok((None, false));
        }
        return Err(DownloadError::HttpStatus {
            status: status.as_u16(),
            url: url.to_string(),
        });
    }

    let size = resp
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok());

    let accept_ranges = resp
        .headers()
        .get(header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("bytes"))
        .unwrap_or(false);

    Ok((size, accept_ranges))
}

/// 下载一个分片（指定 range）。
pub async fn download_segment(
    client: &Client,
    url: &str,
    start: u64,
    end: u64,
    config: &DownloadConfig,
) -> DownloadResult<Bytes> {
    let mut req = client.get(url);
    let mut headers = build_headers(config);

    // Range 头
    let range_val = format!("bytes={}-{}", start, end);
    if let Ok(v) = HeaderValue::from_str(&range_val) {
        headers.insert(RANGE, v);
    }

    // 认证
    if let Some(ref auth) = config.auth.basic {
        req = req.basic_auth(&auth.0, Some(&auth.1));
    }
    if let Some(ref token) = config.auth.bearer {
        req = req.bearer_auth(token);
    }

    let resp = req.headers(headers).send().await?;

    let status = resp.status();
    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(DownloadError::HttpStatus {
            status: status.as_u16(),
            url: url.to_string(),
        });
    }

    let bytes = resp.bytes().await?;
    Ok(bytes)
}

/// 完整下载（不分片，主要用于小文件或不支持 Range 的服务器）。
pub async fn download_full(
    client: &Client,
    url: &str,
    config: &DownloadConfig,
) -> DownloadResult<Bytes> {
    let mut req = client.get(url);
    let headers = build_headers(config);

    // 认证
    if let Some(ref auth) = config.auth.basic {
        req = req.basic_auth(&auth.0, Some(&auth.1));
    }
    if let Some(ref token) = config.auth.bearer {
        req = req.bearer_auth(token);
    }

    let resp = req.headers(headers).send().await?;

    let status = resp.status();
    if !status.is_success() {
        return Err(DownloadError::HttpStatus {
            status: status.as_u16(),
            url: url.to_string(),
        });
    }

    let bytes = resp.bytes().await?;
    Ok(bytes)
}
