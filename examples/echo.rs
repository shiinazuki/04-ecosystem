//! 第 7 步 第一段：最小可跑的 TCP 服务 —— accept 循环 + Framed 分帧 + 回声。
//!
//! ```bash
//! cargo run --example echo          # 一个终端跑服务
//! nc 127.0.0.1 9000                 # 另一个终端连上去，随便打字
//! ```

use std::net::SocketAddr;

use anyhow::Context;
use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{info, warn};

const ADDR: &str = "127.0.0.1:9000";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let listener = TcpListener::bind(ADDR)
        .await
        .with_context(|| format!("无法监听 {ADDR}"))?;
    info!(addr = ADDR, "服务已启动，Ctrl-C 退出");

    loop {
        let (stream, addr) = listener.accept().await.context("accept 失败")?;

        tokio::spawn(async move {
            if let Err(err) = handle(stream, addr).await {
                warn!(%addr, %err, "连接处理失败");
            }
            info!(%addr, "连接已关闭");
        });
    }
}

/// `#[instrument]` 给这条连接开一个 span，之后所有日志自动带上 peer 字段。
#[tracing::instrument(skip_all, fields(peer = %addr))]
async fn handle(stream: TcpStream, addr: SocketAddr) -> anyhow::Result<()> {
    info!("new accept");

    let mut framed = Framed::new(stream, LinesCodec::new());
    framed.send("欢迎，随便打点什么（Ctrl-C 断开）").await?;

    while let Some(line) = framed.next().await {
        let line = line.context("read line failed")?;
        info!(len = line.len(), "收到");
        framed.send(format!("你说的是：{line}")).await?;
    }

    Ok(())
}
