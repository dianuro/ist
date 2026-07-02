use std::time::Duration;

/// 代理配置。
#[derive(Debug, Clone, Default)]
pub struct ProxyConfig {
    /// 代理 URL，如 `socks5://127.0.0.1:1080`
    pub url: Option<String>,
}

/// 认证配置。
#[derive(Debug, Clone, Default)]
pub struct AuthConfig {
    /// Basic Auth 用户名:密码
    pub basic: Option<(String, String)>,
    /// Bearer Token
    pub bearer: Option<String>,
}

/// 单个下载任务的完整配置。
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// 输出文件路径（None 则从 URL 推断）
    pub output: Option<String>,
    /// 分片并发连接数
    pub connections: u32,
    /// 超时时间
    pub timeout: Duration,
    /// 启用断点续传
    pub resume: bool,
    /// 代理配置
    pub proxy: ProxyConfig,
    /// 认证配置
    pub auth: AuthConfig,
    /// 自定义请求头
    pub headers: Vec<(String, String)>,
    /// Cookie 字符串
    pub cookie: Option<String>,
    /// 分片大小（字节），超过此值才启用分片
    pub segment_threshold: u64,
    /// 每个分片的大小（字节）
    pub segment_size: u64,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            output: None,
            connections: 4,
            timeout: Duration::from_secs(30),
            resume: false,
            proxy: ProxyConfig::default(),
            auth: AuthConfig::default(),
            headers: Vec::new(),
            cookie: None,
            segment_threshold: 10 * 1024 * 1024,   // 10 MB
            segment_size: 10 * 1024 * 1024,         // 10 MB
        }
    }
}
