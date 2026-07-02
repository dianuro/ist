use crate::config::DownloadConfig;
use crate::error::{DownloadError, DownloadResult};
use bytes::Bytes;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// FTP 连接状态。
struct FtpConnection {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl FtpConnection {
    fn connect(host: &str, port: u16, timeout: Duration) -> DownloadResult<Self> {
        let addr = format!("{}:{}", host, port)
            .to_socket_addrs()
            .map_err(|e| DownloadError::Protocol(format!("DNS 解析失败: {e}")))?
            .next()
            .ok_or_else(|| DownloadError::Protocol("无法解析主机名".into()))?;

        let stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| DownloadError::Protocol(format!("连接失败: {e}")))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        let mut conn = Self {
            stream,
            buffer: Vec::with_capacity(4096),
        };

        // 读取欢迎消息
        conn.read_response()?;

        Ok(conn)
    }

    fn send_command(&mut self, cmd: &str) -> DownloadResult<()> {
        let line = format!("{}\r\n", cmd);
        self.stream.write_all(line.as_bytes())
            .map_err(|e| DownloadError::Protocol(format!("发送命令失败: {e}")))?;
        Ok(())
    }

    fn read_response(&mut self) -> DownloadResult<String> {
        self.buffer.clear();
        loop {
            let mut byte = [0u8; 1];
            match self.stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    self.buffer.push(byte[0]);
                    // 检查是否为完整响应行（以 \r\n 结尾）
                    if self.buffer.len() >= 2
                        && self.buffer[self.buffer.len() - 2..] == [b'\r', b'\n']
                    {
                        // 检查是否为单行响应或多行响应的最后一行
                        // 多行响应：每行以 "NNN-" 开头，最后一行以 "NNN " 开头
                        if self.buffer.len() >= 4 {
                            let code_start = self.buffer.len() - 4;
                            // 往前找最近的行首
                            let mut line_start = code_start;
                            while line_start > 0 && self.buffer[line_start - 1] != b'\n' {
                                line_start -= 1;
                            }
                            if line_start + 3 < self.buffer.len()
                                && self.buffer[line_start + 3] == b' '
                            {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                Err(e) => return Err(DownloadError::Protocol(format!("读取响应失败: {e}"))),
            }
        }

        let resp = String::from_utf8_lossy(&self.buffer).to_string();
        Ok(resp)
    }

    fn check_ok(&self, resp: &str) -> DownloadResult<()> {
        if resp.len() < 3 {
            return Err(DownloadError::Protocol("FTP 响应过短".into()));
        }
        // 取最后一行
        let last_line = resp.lines().last().unwrap_or("");
        let code: u16 = last_line[..3].parse().unwrap_or(0);
        if code >= 400 {
            return Err(DownloadError::Protocol(format!("FTP 错误: {}", last_line.trim())));
        }
        Ok(())
    }

    fn login(&mut self, user: &str, pass: &str) -> DownloadResult<()> {
        self.send_command(&format!("USER {}", user))?;
        let resp = self.read_response()?;
        self.check_ok(&resp)?;

        self.send_command(&format!("PASS {}", pass))?;
        let resp = self.read_response()?;
        self.check_ok(&resp)?;
        Ok(())
    }

    fn passive_mode(&mut self) -> DownloadResult<(String, u16)> {
        self.send_command("PASV")?;
        let resp = self.read_response()?;
        self.check_ok(&resp)?;

        // PASV 响应格式: 227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)
        let paren_start = resp.find('(').ok_or_else(|| {
            DownloadError::Protocol("PASV 响应格式错误".into())
        })?;
        let paren_end = resp.find(')').ok_or_else(|| {
            DownloadError::Protocol("PASV 响应格式错误".into())
        })?;

        let parts: Vec<u8> = resp[paren_start + 1..paren_end]
            .split(',')
            .map(|s| s.trim().parse().unwrap_or(0))
            .collect();

        if parts.len() < 6 {
            return Err(DownloadError::Protocol("PASV 地址格式错误".into()));
        }

        let ip = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);
        let port = (parts[4] as u16) * 256 + parts[5] as u16;

        Ok((ip, port))
    }

    fn data_connect(&mut self, timeout: Duration) -> DownloadResult<TcpStream> {
        let (ip, port) = self.passive_mode()?;
        let addr = format!("{}:{}", ip, port)
            .to_socket_addrs()
            .map_err(|e| DownloadError::Protocol(format!("数据连接 DNS 失败: {e}")))?
            .next()
            .ok_or_else(|| DownloadError::Protocol("无法解析数据连接地址".into()))?;

        let data_stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| DownloadError::Protocol(format!("数据连接失败: {e}")))?;
        data_stream.set_read_timeout(Some(timeout))?;

        Ok(data_stream)
    }

    fn file_size(&mut self, path: &str) -> DownloadResult<Option<u64>> {
        self.send_command(&format!("SIZE {}", path))?;
        let resp = self.read_response()?;

        if resp.starts_with("213") {
            let size: u64 = resp[3..].trim().parse().unwrap_or(0);
            Ok(Some(size))
        } else {
            Ok(None) // 部分服务器不支持 SIZE
        }
    }

    fn rest(&mut self, offset: u64) -> DownloadResult<bool> {
        self.send_command(&format!("REST {}", offset))?;
        let resp = self.read_response()?;
        // 350 表示支持 REST
        Ok(resp.starts_with("350"))
    }

    fn retrieve(&mut self, path: &str) -> DownloadResult<TcpStream> {
        let data_stream = self.data_connect(Duration::from_secs(10))?;
        self.send_command(&format!("RETR {}", path))?;
        let resp = self.read_response()?;
        self.check_ok(&resp)?;
        Ok(data_stream)
    }

    #[allow(dead_code)]
    fn quit(&mut self) -> DownloadResult<()> {
        let _ = self.send_command("QUIT");
        Ok(())
    }
}

/// 解析 FTP URL: ftp://user:pass@host:port/path
fn parse_ftp_url(url: &str) -> DownloadResult<(String, String, String, u16, String)> {
    let rest = url
        .strip_prefix("ftp://")
        .ok_or_else(|| DownloadError::InvalidUrl("不是 FTP URL".into()))?;

    let (user, pass, hostport);
    if let Some(at_pos) = rest.find('@') {
        let userinfo = &rest[..at_pos];
        hostport = &rest[at_pos + 1..];
        if let Some(colon_pos) = userinfo.find(':') {
            user = userinfo[..colon_pos].to_string();
            pass = userinfo[colon_pos + 1..].to_string();
        } else {
            user = userinfo.to_string();
            pass = String::new();
        }
    } else {
        user = "anonymous".to_string();
        pass = "anonymous@".to_string();
        hostport = rest;
    };

    let (host, path) = if let Some(slash_pos) = hostport.find('/') {
        (&hostport[..slash_pos], &hostport[slash_pos..])
    } else {
        (hostport, "/")
    };

    let (host_str, port) = if let Some(colon_pos) = host.find(':') {
        let p: u16 = host[colon_pos + 1..].parse().unwrap_or(21);
        (&host[..colon_pos], p)
    } else {
        (host, 21u16)
    };

    Ok((user, pass, host_str.to_string(), port, path.to_string()))
}

/// 探测 FTP 文件信息。
pub async fn probe(
    url: &str,
    config: &DownloadConfig,
) -> DownloadResult<(Option<u64>, bool)> {
    let (user, pass, host, port, path) = parse_ftp_url(url)?;
    let timeout = config.timeout;

    let (size, supports_rest) = tokio::task::spawn_blocking(move || -> DownloadResult<_> {
        let mut conn = FtpConnection::connect(&host, port, timeout)?;
        conn.login(&user, &pass)?;
        let size = conn.file_size(&path)?;
        // 检查 REST 支持
        let supports_rest = if let Some(s) = size {
            if s > 0 {
                conn.rest(s - 1).unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        };
        let _ = conn.quit();
        Ok((size, supports_rest))
    })
    .await
    .map_err(|e| DownloadError::Protocol(format!("FTP 任务失败: {e}")))??;

    Ok((size, supports_rest))
}

/// 下载 FTP 文件的指定 range。
pub async fn download_range(
    url: &str,
    start: u64,
    _end: u64,
    config: &DownloadConfig,
) -> DownloadResult<Bytes> {
    let (user, pass, host, port, path) = parse_ftp_url(url)?;
    let timeout = config.timeout;

    let bytes = tokio::task::spawn_blocking(move || -> DownloadResult<Bytes> {
        let mut conn = FtpConnection::connect(&host, port, timeout)?;
        conn.login(&user, &pass)?;

        if start > 0 {
            conn.rest(start)?;
        }

        let mut data_stream = conn.retrieve(&path)?;
        let mut buf = Vec::new();
        data_stream
            .read_to_end(&mut buf)
            .map_err(|e| DownloadError::Protocol(format!("读取 FTP 数据失败: {e}")))?;

        let _ = conn.quit();
        Ok(Bytes::from(buf))
    })
    .await
    .map_err(|e| DownloadError::Protocol(format!("FTP 下载任务失败: {e}")))??;

    Ok(bytes)
}

/// 完整下载 FTP 文件（不分片）。
pub async fn download_full(
    url: &str,
    config: &DownloadConfig,
) -> DownloadResult<Bytes> {
    download_range(url, 0, 0, config).await
}
