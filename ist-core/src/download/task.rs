use crate::config::DownloadConfig;
use crate::download::segment::{
    self, cleanup_meta, load_meta, merge_segments, save_meta, segment_file_path, split_segments,
    DownloadMeta, SegmentStatus,
};
use crate::error::{DownloadError, DownloadResult};
use crate::progress::{ProgressInfo, ProgressReporter, SilentProgress};
use crate::protocol::{http as http_proto, ftp as ftp_proto, FileInfo};
use std::sync::Arc;
use std::time::Instant;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tracing;

/// 从 URL 推断输出文件名。
pub fn filename_from_url(url: &str) -> String {
    let url_path = url.split('?').next().unwrap_or(url);
    let basename = url_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("download");
    if basename.is_empty() {
        "download".to_string()
    } else {
        basename.to_string()
    }
}

/// 判断是否为 FTP URL。
fn is_ftp_url(url: &str) -> bool {
    url.trim().to_lowercase().starts_with("ftp://")
}

/// 运行单个下载任务。
pub async fn run(url: &str, config: DownloadConfig) -> DownloadResult<()> {
    run_with_progress(url, config, &SilentProgress).await
}

/// 带进度回调的运行。
pub async fn run_with_progress(
    url: &str,
    config: DownloadConfig,
    progress: &dyn ProgressReporter,
) -> DownloadResult<()> {
    let url = url.trim().to_string();
    let output_path = config
        .output
        .clone()
        .unwrap_or_else(|| filename_from_url(&url));

    // 检查断点续传
    if config.resume {
        if let Some(meta) = load_meta(&output_path).await.map_err(DownloadError::Io)? {
            if meta.is_complete() {
                tracing::info!("文件已完整下载: {}", output_path);
                progress.on_complete(&output_path);
                return Ok(());
            }
            // 继续未完成的分片
            return resume_download(meta, &config, progress).await;
        }
    }

    // 全新下载
    fresh_download(&url, &output_path, &config, progress).await
}

/// 全新下载。
async fn fresh_download(
    url: &str,
    output_path: &str,
    config: &DownloadConfig,
    progress: &dyn ProgressReporter,
) -> DownloadResult<()> {
    let file_info = if is_ftp_url(url) {
        let (size, accept_ranges) = ftp_proto::probe(url, config).await?;
        FileInfo { size, accept_ranges }
    } else {
        let client = http_proto::build_client(config)?;
        let (size, accept_ranges) = http_proto::probe(&client, url, config).await?;
        FileInfo { size, accept_ranges }
    };

    let segments = split_segments(
        file_info.size,
        config.segment_size,
        config.segment_threshold,
        config.connections,
    );

    let meta = DownloadMeta {
        url: url.to_string(),
        total_size: file_info.size,
        segments,
        output_path: output_path.to_string(),
    };

    save_meta(&meta).await.map_err(DownloadError::Io)?;

    // 小文件：不分片，直接下载
    if meta.segments.len() == 1
        && meta.segments[0].length > 0
    {
        let is_small = if let Some(total) = meta.total_size {
            total <= config.segment_threshold
        } else {
            true
        };

        if is_small {
            let bytes = if is_ftp_url(url) {
                ftp_proto::download_full(url, config).await?
            } else {
                let client = http_proto::build_client(config)?;
                http_proto::download_full(&client, url, config).await?
            };
            fs::write(output_path, &bytes).await.map_err(DownloadError::Io)?;
            cleanup_meta(output_path).await.map_err(DownloadError::Io)?;
            progress.on_complete(output_path);
            return Ok(());
        }
    }

    download_segments(url, meta, config, progress).await
}

/// 断点续传。
async fn resume_download(
    meta: DownloadMeta,
    config: &DownloadConfig,
    progress: &dyn ProgressReporter,
) -> DownloadResult<()> {
    tracing::info!(
        "恢复下载: {} (已完成 {} / {})",
        meta.output_path,
        meta.completed_bytes(),
        meta.total_size.unwrap_or(0),
    );
    let url = meta.url.clone();
    download_segments(&url, meta, config, progress).await
}

/// 并发下载所有分片。
async fn download_segments(
    url: &str,
    mut meta: DownloadMeta,
    config: &DownloadConfig,
    progress: &dyn ProgressReporter,
) -> DownloadResult<()> {
    let total_size = meta.total_size.unwrap_or(0);
    let output_path = meta.output_path.clone();
    let url = url.to_string();
    let semaphore = Arc::new(Semaphore::new(config.connections as usize));
    let start_time = Instant::now();

    let mut handles = Vec::new();

    for segment in meta.segments.iter() {
        if segment.status == SegmentStatus::Completed {
            continue;
        }

        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let url = url.clone();
        let config = config.clone();
        let meta_dir = segment::meta_dir(&output_path);
        let seg = segment.clone();

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let result = if is_ftp_url(&url) {
                let end = if seg.length > 0 {
                    seg.offset + seg.length - 1
                } else {
                    0
                };
                ftp_proto::download_range(&url, seg.offset, end, &config).await
                    .map_err(|e| (seg.index, format!("FTP 分片下载失败: {e}")))
            } else {
                let client = http_proto::build_client(&config)
                    .map_err(|e| (seg.index, format!("构建 HTTP 客户端失败: {e}")))?;
                let end = if seg.length > 0 {
                    seg.offset + seg.length - 1
                } else {
                    0
                };
                http_proto::download_segment(&client, &url, seg.offset, end, &config).await
                    .map_err(|e| (seg.index, format!("HTTP 分片下载失败: {e}")))
            };

            match result {
                Ok(bytes) => {
                    let part_path = segment_file_path(&meta_dir, seg.index);
                    // 确保目录存在
                    if let Some(parent) = part_path.parent() {
                        let _ = fs::create_dir_all(parent).await;
                    }
                    let mut file = fs::File::create(&part_path).await
                        .map_err(|e| (seg.index, format!("创建分片文件失败: {e}")))?;
                    file.write_all(&bytes).await
                        .map_err(|e| (seg.index, format!("写入分片失败: {e}")))?;
                    Ok((seg.index, bytes.len() as u64))
                }
                Err(e) => Err(e),
            }
        });

        handles.push(handle);
    }

    // 等待所有分片完成
    for handle in handles {
        match handle.await {
            Ok(Ok((index, size))) => {
                // 更新 meta
                if let Some(seg) = meta.segments.iter_mut().find(|s| s.index == index) {
                    seg.status = SegmentStatus::Completed;
                    seg.downloaded = size;
                }
                save_meta(&meta).await.map_err(DownloadError::Io)?;

                let elapsed = start_time.elapsed();
                let downloaded = meta.completed_bytes();
                let speed = if elapsed.as_secs_f64() > 0.0 {
                    downloaded as f64 / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                progress.on_progress(&ProgressInfo {
                    downloaded,
                    total: total_size,
                    speed,
                    elapsed,
                });
            }
            Ok(Err((_index, err))) => {
                progress.on_error(&err);
                return Err(DownloadError::RetryExhausted(err));
            }
            Err(e) => {
                return Err(DownloadError::Protocol(format!("任务异常: {e}")));
            }
        }
    }

    // 合并分片
    if meta.is_complete() {
        tracing::info!("所有分片下载完成，开始合并...");
        merge_segments(&meta.segments, &output_path).await
            .map_err(DownloadError::Io)?;
        cleanup_meta(&output_path).await.map_err(DownloadError::Io)?;
        progress.on_complete(&output_path);
    }

    Ok(())
}
