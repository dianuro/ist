pub mod config;
pub mod download;
pub mod error;
pub mod progress;
pub mod protocol;

use config::DownloadConfig;
use error::DownloadResult;

/// 下载单个文件。
pub async fn download(url: &str, config: DownloadConfig) -> DownloadResult<()> {
    download::scheduler::download_single(url, config).await
}

/// 批量下载多个文件。
pub async fn download_batch(
    urls: &[String],
    config: DownloadConfig,
    max_concurrent: usize,
) -> Vec<(String, DownloadResult<()>)> {
    download::scheduler::run_batch(urls, config, max_concurrent).await
}
