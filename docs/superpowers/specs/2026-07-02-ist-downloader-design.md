# ist 下载器 — 设计文档

> 日期：2026-07-02
> 状态：草稿

## 1. 概述

`ist` 是一个 Rust（edition 2024）高性能多线程下载器，定位为**通用下载工具**，支持大文件分片并发下载和小文件批量并发。支持 HTTP/HTTPS 和 FTP 协议，支持断点续传。

**性能目标**：单连接跑满带宽，多连接进一步加速。

**目标平台**：Linux 优先（跨平台待定）。

## 2. 项目结构

采用 workspace 方案，核心库与 CLI 分离：

```
ist/
├── Cargo.toml                 # workspace 根
├── Cargo.lock
├── ist-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs             # 公开 API
│       ├── config/
│       │   ├── mod.rs
│       │   └── types.rs       # 配置模型
│       ├── protocol/
│       │   ├── mod.rs
│       │   ├── http.rs        # HTTP/HTTPS 实现
│       │   └── ftp.rs         # FTP 实现
│       ├── download/
│       │   ├── mod.rs
│       │   ├── task.rs        # 下载任务生命周期
│       │   ├── segment.rs     # 分片管理
│       │   └── scheduler.rs   # 全局调度器
│       ├── error.rs           # 错误类型
│       └── progress.rs        # 进度回调
├── ist-cli/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs            # CLI 入口
├── docs/
│   └── superpowers/
│       └── specs/
│           └── 2026-07-02-ist-downloader-design.md
└── .gitignore
```

### 依赖栈

**ist-core**:
- `tokio` — 异步运行时
- `reqwest` — HTTP 客户端
- `bytes` — 字节缓冲区
- `thiserror` — 错误处理
- `serde` / `serde_json` — 元数据序列化

**ist-cli**:
- `clap` — 参数解析
- `indicatif` — 进度条
- `tokio` — 异步运行时
- `ist-core` — 本地依赖

## 3. 核心架构

### 3.1 下载引擎数据流

```
用户调用 download(url, config)
       │
       ▼
  Scheduler.acquire()           ← 获取可用连接槽
       │
       ▼
  ProtocolDetector.detect(url)  ← 根据 URL scheme 选择 HTTP/FTP
       │
       ▼
  [HTTP]                          [FTP]
   HEAD 请求获取文件大小          FTP SIZE/PASV 获取文件大小
   检查 Range 支持               检查 REST 支持
       │                              │
       ▼                              ▼
  SegmentManager.split()        SegmentManager.split()
       │                              │
       ▼                              ▼
  Worker pool (n 个并行)        Worker pool (n 个并行)
  每个 worker 下载自己的分片      每个 worker 下载自己的分片
  写入 .part.N 临时文件           写入 .part.N 临时文件
       │                              │
       ▼                              ▼
  SegmentManager.merge()          SegmentManager.merge()
       │                              │
       ▼
  最终文件输出
```

### 3.2 模块职责

| 模块 | 职责 |
|------|------|
| `config/types.rs` | `DownloadConfig`、`ProxyConfig`、`AuthConfig` 等配置模型 |
| `protocol/http.rs` | HTTP/HTTPS 的 HEAD/GET 请求、Range 处理、重定向处理 |
| `protocol/ftp.rs` | FTP 的 LIST/RETR/SIZE/REST 命令、被动模式、断点续传 |
| `download/task.rs` | 单个下载任务的全生命周期：探测→分片→下载→合并 |
| `download/segment.rs` | 分片信息的表示、拆分策略、进度跟踪、合并逻辑 |
| `download/scheduler.rs` | 全局并发控制（最大连接数、Worker 池管理）、批量任务调度 |
| `error.rs` | 统一的 `DownloadError` 枚举 |
| `progress.rs` | 进度回调 trait，供 CLI/TUI 等界面实现 |

## 4. 分片与断点续传

### 4.1 分片策略

- **大文件（> 分片阈值）**：通过 HTTP `Range` 头分片，各分片独立下载
- **小文件（≤ 分片阈值）**：不分片，单线程下载
- **分片阈值**：10 MB（文件 ≤ 10MB 不分片，> 10MB 按分片大小切分）
- **默认分片大小**：10 MB（每个分片的字节数，可配置）
- **默认并发连接数**：4（可配置，`-n` 参数）

### 4.2 断点续传

- 下载进度持久化到 `<output>.ist/` 目录
- 元数据文件：`<output>.ist/meta.json`
  - 每个分片：`offset`, `length`, `downloaded`, `status`
- 重启流程：
  1. 检查 `<output>.ist/meta.json` 是否存在
  2. 若存在且未完成，恢复未完成的分片
  3. 若已完成，跳过（文件已存在）
- 合并完成后清理 `.ist/` 目录

### 4.3 合并策略

- 所有分片完成后，按 offset 顺序合并
- 使用 `tokio::io` 异步流式合并，避免内存占用过大
- 合并完成后删除临时 `.part.N` 文件

## 5. 协议适配

### 5.1 HTTP/HTTPS

- 使用 `reqwest` 作为 HTTP 客户端
- HEAD 请求获取文件元信息（Content-Length、Accept-Ranges）
- GET + Range 分片下载
- 支持重定向自动跟随
- 支持代理（HTTP / SOCKS5 — reqwest 原生支持）
- 支持 Basic Auth、Bearer Token、自定义头、Cookie

### 5.2 FTP

- 使用自定义 FTP 协议实现（或选型已存在的 `async-ftp` crate）
- `SIZE` 命令获取文件大小
- `REST` 命令实现断点续传
- 被动模式（PASV）数据传输
- 支持代理（可选）

## 6. CLI 接口

### 6.1 使用方式

```
Usage: ist [OPTIONS] <URL|FILE>

Arguments:
  <URL|FILE>    下载地址，或包含 URL 列表的文件路径（配合 -i）

Options:
  -o, --output <PATH>          指定输出路径（默认：URL 文件名）
  -n, --connections <NUM>      分片并发连接数 [默认: 4]
  -j, --jobs <NUM>             多文件批量下载并发数 [默认: 2]
  -i, --input-file             参数为 URL 列表文件路径
  -r, --resume                 启用断点续传
  --proxy <URL>                代理地址 (http:// / socks5://)
  --auth <USER:PASS>           Basic 认证
  --header <KEY:VALUE>         自定义请求头（可重复）
  --cookie <STRING>            Cookie
  --timeout <SECONDS>          超时 [默认: 30]
  -h, --help                   帮助
  -V, --version                版本
```

### 6.2 进度显示

- 单文件下载：进度条（速度、ETA、已下载/总大小）
- 批量下载：总进度 + 每个文件独立进度

### 6.3 错误处理

- 网络错误自动重试（最多 3 次，指数退避）
- 磁盘空间不足提前检测
- 服务器不支持 Range 时自动降级单线程

## 7. 性能考虑

- 通过 tokio 异步 I/O 最大化并发
- 分片写入独立临时文件，避免锁竞争
- 分段合并使用流式读写，内存效率高
- 连接池复用 keep-alive 连接
- 对大文件使用按需分配缓冲区，避免大块内存浪费

## 8. 未包含（但未来可考虑）

- BitTorrent / 磁力链接支持
- 多平台（macOS/Windows）支持
- TUI 界面
- 作为库提供给第三方使用
- 配置文件持久化
- 下载任务队列管理
