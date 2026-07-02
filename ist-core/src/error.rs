use std::path::PathBuf;
use thiserror::Error;

/// 下载过程中可能发生的所有错误。
#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("网络错误: {0}")]
    Network(#[from] reqwest::Error),

    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("协议错误: {0}")]
    Protocol(String),

    #[error("服务器返回错误状态: {status} - {url}")]
    HttpStatus { status: u16, url: String },

    #[error("文件不存在: {0}")]
    FileNotFound(PathBuf),

    #[error("磁盘空间不足: 需要 {needed} 但仅 {available}")]
    DiskSpace { needed: u64, available: u64 },

    #[error("服务器不支持 Range 请求，降级为单线程")]
    RangeNotSupported,

    #[error("URL 格式错误: {0}")]
    InvalidUrl(String),

    #[error("重试耗尽: {0}")]
    RetryExhausted(String),

    #[error("任务取消")]
    Cancelled,
}

/// 带上下文的结果类型。
pub type DownloadResult<T> = Result<T, DownloadError>;
