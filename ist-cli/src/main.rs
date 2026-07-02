use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle, MultiProgress};
use ist_core::config::{AuthConfig, DownloadConfig, ProxyConfig};
use ist_core::download::scheduler;
use ist_core::progress::{ProgressInfo, ProgressReporter};

/// ist — 高性能多线程下载器
#[derive(Parser, Debug)]
#[command(name = "ist", version, about)]
struct Cli {
    /// 下载 URL 或文件路径（配合 -i 使用）
    url: Option<String>,

    /// 指定输出路径
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// 分片并发连接数
    #[arg(short = 'n', long, default_value = "4")]
    connections: u32,

    /// 多文件批量下载并发数
    #[arg(short = 'j', long, default_value = "2")]
    jobs: usize,

    /// 从文件读取 URL 列表
    #[arg(short = 'i', long)]
    input_file: Option<PathBuf>,

    /// 启用断点续传
    #[arg(short = 'r', long)]
    resume: bool,

    /// 代理地址 (http:// / socks5://)
    #[arg(long)]
    proxy: Option<String>,

    /// 基本认证 (user:pass)
    #[arg(long)]
    auth: Option<String>,

    /// 自定义请求头 (可重复)
    #[arg(long, action = clap::ArgAction::Append)]
    header: Vec<String>,

    /// Cookie 字符串
    #[arg(long)]
    cookie: Option<String>,

    /// 超时时间（秒）
    #[arg(long, default_value = "30")]
    timeout: u64,
}

/// CLI 进度条实现。
struct CliProgress {
    pb: ProgressBar,
}

impl CliProgress {
    fn new(pb: ProgressBar) -> Self {
        Self { pb }
    }
}

impl ProgressReporter for CliProgress {
    fn on_progress(&self, info: &ProgressInfo) {
        let speed = info.speed;
        let speed_str = if speed >= 1024.0 * 1024.0 {
            format!("{:.1} MB/s", speed / (1024.0 * 1024.0))
        } else if speed >= 1024.0 {
            format!("{:.1} KB/s", speed / 1024.0)
        } else {
            format!("{:.0} B/s", speed)
        };

        if info.total > 0 {
            let pct = (info.downloaded as f64 / info.total as f64) * 100.0;
            let downloaded = indicatif::HumanBytes(info.downloaded);
            let total = indicatif::HumanBytes(info.total);
            self.pb.set_position(info.downloaded);
            self.pb.set_length(info.total);
            self.pb.set_message(format!(
                "{}  {} / {}  {:.1}%",
                speed_str, downloaded, total, pct
            ));
        } else {
            self.pb.set_position(info.downloaded);
            self.pb.set_message(format!(
                "{}  {}",
                speed_str,
                indicatif::HumanBytes(info.downloaded)
            ));
        }
    }

    fn on_complete(&self, path: &str) {
        self.pb.finish_with_message(format!("✓ 完成: {}", path));
    }

    fn on_error(&self, err: &str) {
        self.pb.finish_with_message(format!("✗ 错误: {}", err));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 解析认证
    let auth = if let Some(auth_str) = &cli.auth {
        if let Some(colon_pos) = auth_str.find(':') {
            let user = auth_str[..colon_pos].to_string();
            let pass = auth_str[colon_pos + 1..].to_string();
            AuthConfig {
                basic: Some((user, pass)),
                ..Default::default()
            }
        } else {
            AuthConfig {
                bearer: Some(auth_str.clone()),
                ..Default::default()
            }
        }
    } else {
        AuthConfig::default()
    };

    // 解析请求头
    let headers: Vec<(String, String)> = cli
        .header
        .iter()
        .filter_map(|h| {
            let mut parts = h.splitn(2, ':');
            let key = parts.next()?.trim().to_string();
            let value = parts.next()?.trim().to_string();
            Some((key, value))
        })
        .collect();

    let config = DownloadConfig {
        output: cli.output,
        connections: cli.connections,
        timeout: Duration::from_secs(cli.timeout),
        resume: cli.resume,
        proxy: ProxyConfig { url: cli.proxy },
        auth,
        headers,
        cookie: cli.cookie,
        ..Default::default()
    };

    // 收集 URL
    let urls = if let Some(file) = cli.input_file {
        // 从文件读取 URL 列表
        let content = std::fs::read_to_string(&file)?;
        content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect::<Vec<_>>()
    } else if let Some(url) = cli.url {
        vec![url]
    } else {
        // 无参数，显示帮助
        use clap::CommandFactory;
        Cli::command().print_help()?;
        return Ok(());
    };

    // 执行下载
    if urls.len() == 1 {
        // 单文件下载
        let pb = ProgressBar::new(0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} @ {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.enable_steady_tick(Duration::from_millis(100));

        let reporter = CliProgress::new(pb);
        match scheduler::download_single(&urls[0], config).await {
            Ok(()) => {}
            Err(e) => {
                reporter.on_error(&e.to_string());
                eprintln!("\n错误: {e}");
                std::process::exit(1);
            }
        }
    } else {
        // 批量下载
        let multi = MultiProgress::new();
        let mut reporters = Vec::new();

        for url in &urls {
            let pb = multi.add(ProgressBar::new(0));
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] [{bar:40.green/blue}] {bytes}  {msg}")
                    .unwrap()
                    .progress_chars("#>-"),
            );
            pb.enable_steady_tick(Duration::from_millis(100));
            reporters.push((url.clone(), CliProgress::new(pb)));
        }

        let results = scheduler::run_batch(&urls, config, cli.jobs).await;

        for ((url, reporter), (_res_url, result)) in reporters.into_iter().zip(results.into_iter()) {
            match result {
                Ok(()) => reporter.on_complete(&url),
                Err(e) => {
                    reporter.on_error(&e.to_string());
                    eprintln!("{} 失败: {e}", url);
                }
            }
        }
    }

    Ok(())
}
