# ist — 高性能多线程下载器

`ist` 是一个基于 Rust 的高性能多线程下载器，支持 HTTP/HTTPS 和 FTP 协议。

## 特性

- **大文件分片并发** — 将大文件拆分为多个分片并行下载，大幅提升速度
- **小文件批量并发** — 同时下载多个小文件，充分利用带宽
- **断点续传** — 支持中断后恢复下载
- **HTTP/HTTPS + FTP** — 多协议支持
- **代理支持** — HTTP 代理和 SOCKS5 代理
- **认证** — Basic Auth、Bearer Token
- **自定义请求头 / Cookie** — 灵活配置下载请求
- **实时进度显示** — 进度条、速度、ETA

## 安装

```bash
# 从源码安装
git clone <repo-url>
cd ist
cargo install --path .

# 或直接运行
cargo run -- <url>
```

## 使用

```bash
# 下载单个文件
ist https://example.com/bigfile.iso

# 8 分片并发下载
ist -n 8 -o output.iso https://example.com/bigfile.iso

# 批量下载
ist -i urls.txt -j 4

# 断点续传
ist --resume https://example.com/bigfile.iso

# 使用 SOCKS5 代理
ist --proxy socks5://127.0.0.1:1080 https://example.com/file

# 带认证下载
ist --auth user:pass https://example.com/private-file

# 自定义请求头
ist --header "User-Agent: ist/0.1" --header "Referer: https://example.com" https://example.com/file

# 带 Cookie
ist --cookie "session=abc123" https://example.com/file
```

## 选项

| 选项 | 说明 |
|------|------|
| `-o, --output <PATH>` | 指定输出路径 |
| `-n, --connections <NUM>` | 分片并发连接数（默认 4） |
| `-j, --jobs <NUM>` | 批量文件并发数（默认 2） |
| `-i, --input-file <FILE>` | 从文件读取 URL 列表 |
| `-r, --resume` | 启用断点续传 |
| `--proxy <URL>` | 代理地址 (http:// / socks5://) |
| `--auth <USER:PASS>` | Basic 认证 |
| `--header <KEY:VALUE>` | 自定义请求头（可重复） |
| `--cookie <STRING>` | Cookie 字符串 |
| `--timeout <SECONDS>` | 超时时间（默认 30） |

## 项目结构

```
ist/
├── ist-core/         # 下载引擎核心库
│   ├── src/
│   │   ├── config/   # 配置模型
│   │   ├── download/ # 分片管理、任务调度
│   │   ├── protocol/ # HTTP/FTP 协议实现
│   │   ├── error.rs  # 错误类型
│   │   └── progress.rs # 进度回调 trait
│   └── tests/        # 集成测试
├── ist-cli/          # 命令行界面
│   └── src/
│       └── main.rs
└── docs/             # 文档
```

## 许可证

MIT
