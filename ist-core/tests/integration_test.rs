use ist_core::download::segment::split_segments;
use ist_core::config::DownloadConfig;

#[test]
fn test_split_segments_small_file() {
    // 小文件（< 10MB）不分片
    let segments = split_segments(Some(5 * 1024 * 1024), 10 * 1024 * 1024, 10 * 1024 * 1024, 4);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].offset, 0);
    assert_eq!(segments[0].length, 5 * 1024 * 1024);
}

#[test]
fn test_split_segments_large_file() {
    // 大文件（50MB）按 10MB 分片
    let segments = split_segments(Some(50 * 1024 * 1024), 10 * 1024 * 1024, 10 * 1024 * 1024, 4);
    assert_eq!(segments.len(), 5);
    assert_eq!(segments[0].offset, 0);
    assert_eq!(segments[4].offset, 40 * 1024 * 1024);
    assert_eq!(segments[4].length, 10 * 1024 * 1024);
}

#[test]
fn test_split_segments_unknown_size() {
    // 未知大小返回一个分片
    let segments = split_segments(None, 10 * 1024 * 1024, 10 * 1024 * 1024, 4);
    assert_eq!(segments.len(), 1);
}

#[test]
fn test_split_segments_exact_threshold() {
    // 等于阈值也不分片
    let segments = split_segments(Some(10 * 1024 * 1024), 10 * 1024 * 1024, 10 * 1024 * 1024, 4);
    assert_eq!(segments.len(), 1);
}

#[test]
fn test_default_config() {
    let config = DownloadConfig::default();
    assert_eq!(config.connections, 4);
    assert_eq!(config.segment_threshold, 10 * 1024 * 1024);
    assert_eq!(config.segment_size, 10 * 1024 * 1024);
    assert!(!config.resume);
}

#[test]
fn test_filename_from_url() {
    let name = ist_core::download::task::filename_from_url(
        "https://example.com/path/to/file.zip",
    );
    assert_eq!(name, "file.zip");
}

#[test]
fn test_filename_from_url_with_query() {
    let name = ist_core::download::task::filename_from_url(
        "https://example.com/file.txt?token=abc&dl=1",
    );
    assert_eq!(name, "file.txt");
}

#[test]
fn test_filename_from_url_root() {
    let name = ist_core::download::task::filename_from_url("https://example.com/");
    assert_eq!(name, "example.com"); // 域名作为文件名
}

#[test]
fn test_filename_from_ftp_url() {
    let name = ist_core::download::task::filename_from_url(
        "ftp://ftp.example.com/pub/software/latest.iso",
    );
    assert_eq!(name, "latest.iso");
}
