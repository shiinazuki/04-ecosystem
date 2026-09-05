//! RUST ECOSYSTEM
//!
//! 可执行入口：初始化日志、收口错误、把结果写到 stdout，业务逻辑在 `src/lib.rs`。

use anyhow::Context;
use ecosystem::{ConfigError, load_port};
use tracing::{info, warn};

mod telemetry;

const DEFAULT_PORT: u16 = 8080;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init("info");

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_owned());

    let port = match load_port(&path).await {
        Ok(port) => port,
        Err(ConfigError::NotFound { path }) => {
            warn!(path = %path.display(), "配置文件不存在, 使用默认端口");
            DEFAULT_PORT
        }
        Err(err) => return Err(err).with_context(|| format!("加载配置 {path} 失败")),
    };

    info!("port is : {port}");

    Ok(())
}
