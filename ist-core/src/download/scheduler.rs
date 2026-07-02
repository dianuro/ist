use crate::config::DownloadConfig;
use crate::download::task;
use crate::error::DownloadResult;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// 批量下载多个文件。
///
/// `max_concurrent` 控制同时下载的最大文件数（`-j` 参数）。
pub async fn run_batch(
    urls: &[String],
    config: DownloadConfig,
    max_concurrent: usize,
) -> Vec<(String, DownloadResult<()>)> {
    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let mut handles = Vec::new();

    for url in urls {
        let url = url.clone();
        let config = config.clone();
        let permit = semaphore.clone().acquire_owned().await;

        match permit {
            Ok(permit) => {
                let handle = tokio::spawn(async move {
                    let _permit = permit;
                    let result = task::run(&url, config).await;
                    (url, result)
                });
                handles.push(handle);
            }
            Err(_) => {
                handles.push(tokio::spawn(async move {
                    (url, Err(crate::error::DownloadError::Cancelled))
                }));
            }
        }
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok((url, result)) => results.push((url, result)),
            Err(e) => results.push((
                "unknown".to_string(),
                Err(crate::error::DownloadError::Protocol(format!(
                    "任务执行失败: {e}"
                ))),
            )),
        }
    }

    results
}

/// 单个文件下载的封装入口（由 CLI 调用）。
pub async fn download_single(url: &str, config: DownloadConfig) -> DownloadResult<()> {
    task::run(url, config).await
}
