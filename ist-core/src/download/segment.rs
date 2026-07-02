use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 分片状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SegmentStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

/// 分片信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// 分片索引
    pub index: u32,
    /// 文件中的起始偏移
    pub offset: u64,
    /// 分片长度
    pub length: u64,
    /// 已下载字节数
    pub downloaded: u64,
    /// 状态
    pub status: SegmentStatus,
}

impl Segment {
    pub fn new(index: u32, offset: u64, length: u64) -> Self {
        Self {
            index,
            offset,
            length,
            downloaded: 0,
            status: SegmentStatus::Pending,
        }
    }
}

/// 下载元数据（持久化到磁盘）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMeta {
    pub url: String,
    pub total_size: Option<u64>,
    pub segments: Vec<Segment>,
    pub output_path: String,
}

impl DownloadMeta {
    /// 获取已完成的总字节数。
    pub fn completed_bytes(&self) -> u64 {
        self.segments
            .iter()
            .filter(|s| s.status == SegmentStatus::Completed)
            .map(|s| s.length)
            .sum()
    }

    /// 所有分片是否全部完成。
    pub fn is_complete(&self) -> bool {
        self.segments.iter().all(|s| s.status == SegmentStatus::Completed)
    }

    /// 获取未完成的分片。
    pub fn pending_segments(&self) -> Vec<&Segment> {
        self.segments
            .iter()
            .filter(|s| s.status != SegmentStatus::Completed)
            .collect()
    }
}

/// 根据文件大小和配置生成分片列表。
pub fn split_segments(
    total_size: Option<u64>,
    segment_size: u64,
    threshold: u64,
    _num_connections: u32,
) -> Vec<Segment> {
    let Some(size) = total_size else {
        // 未知大小，返回一个分片
        return vec![Segment::new(0, 0, 0)];
    };

    // 文件小于阈值，不分片
    if size <= threshold {
        return vec![Segment::new(0, 0, size)];
    }

    // 大文件：按分片大小切分
    let mut segments = Vec::new();
    let mut offset = 0u64;
    let mut index = 0u32;

    while offset < size {
        let remaining = size - offset;
        let len = segment_size.min(remaining);
        segments.push(Segment::new(index, offset, len));
        offset += len;
        index += 1;
    }

    segments
}

/// 元数据目录名（基于输出文件名）。
pub fn meta_dir(output_path: &str) -> PathBuf {
    let path = PathBuf::from(output_path);
    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    let parent = path.parent().unwrap_or(Path::new("."));
    parent.join(format!("{}.ist", filename))
}

/// 分片临时文件名。
pub fn segment_file_path(meta_dir: &Path, index: u32) -> PathBuf {
    meta_dir.join(format!("part.{}", index))
}

/// 保存元数据到磁盘。
pub async fn save_meta(meta: &DownloadMeta) -> Result<(), std::io::Error> {
    let dir = meta_dir(&meta.output_path);
    fs::create_dir_all(&dir).await?;
    let json = serde_json::to_string_pretty(meta)?;
    let mut file = fs::File::create(dir.join("meta.json")).await?;
    file.write_all(json.as_bytes()).await?;
    Ok(())
}

/// 从磁盘加载元数据。
pub async fn load_meta(output_path: &str) -> Result<Option<DownloadMeta>, std::io::Error> {
    let dir = meta_dir(output_path);
    let meta_path = dir.join("meta.json");
    if !meta_path.exists() {
        return Ok(None);
    }
    let mut file = fs::File::open(&meta_path).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    let meta: DownloadMeta = serde_json::from_str(&content)?;
    Ok(Some(meta))
}

/// 删除元数据目录。
pub async fn cleanup_meta(output_path: &str) -> Result<(), std::io::Error> {
    let dir = meta_dir(output_path);
    if dir.exists() {
        fs::remove_dir_all(&dir).await?;
    }
    Ok(())
}

/// 合并分片为一个完整文件。
pub async fn merge_segments(
    segments: &[Segment],
    output_path: &str,
) -> Result<(), std::io::Error> {
    let dir = meta_dir(output_path);
    let mut output_file = fs::File::create(output_path).await?;

    for seg in segments {
        let part_path = segment_file_path(&dir, seg.index);
        if !part_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("分片文件不存在: {:?}", part_path),
            ));
        }
        let mut part_file = fs::File::open(&part_path).await?;
        let mut buf = vec![0u8; 64 * 1024]; // 64KB 缓冲区
        loop {
            let n = part_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            output_file.write_all(&buf[..n]).await?;
        }
    }

    Ok(())
}
