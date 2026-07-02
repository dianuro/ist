/// 进度回调信息。
#[derive(Debug, Clone)]
pub struct ProgressInfo {
    /// 已下载字节数
    pub downloaded: u64,
    /// 总大小（未知则为 0）
    pub total: u64,
    /// 速度（字节/秒）
    pub speed: f64,
    /// 已用时间
    pub elapsed: std::time::Duration,
}

/// 进度回调 trait。CLI 或 TUI 实现此 trait 来展示进度。
pub trait ProgressReporter: Send + Sync {
    fn on_progress(&self, info: &ProgressInfo);
    fn on_complete(&self, path: &str);
    fn on_error(&self, err: &str);
}

/// 静默版本（什么也不做）。
pub struct SilentProgress;
impl ProgressReporter for SilentProgress {
    fn on_progress(&self, _info: &ProgressInfo) {}
    fn on_complete(&self, _path: &str) {}
    fn on_error(&self, _err: &str) {}
}
