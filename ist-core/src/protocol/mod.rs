pub mod http;
pub mod ftp;

/// 探测结果：文件是否支持分片及总大小。
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub size: Option<u64>,
    pub accept_ranges: bool,
}
