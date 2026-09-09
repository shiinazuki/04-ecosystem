//! RUST ECOSYSTEM
//!
//! 可执行入口：初始化日志、收口错误、把结果写到 stdout，业务逻辑在 `src/lib.rs`。

use std::io::{self, Write as _};

use anyhow::Context as _;
use ecosystem::{Config, ConfigError, load_config, parse_mode};
use tracing::{info, warn};

mod telemetry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = telemetry::init("info").context("初始化日志失败")?;

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_owned());

    let mut config = match load_config(&path).await {
        Ok(config) => config,
        Err(ConfigError::NotFound { path }) => {
            warn!(path = %path.display(), "配置文件不存在, 使用默认配置");
            Config::default()
        }
        // 其余错误（格式错、端口为 0、读不了）说明用户表态了但出了问题 → 报错退出
        Err(err) => return Err(err).with_context(|| format!("加载配置 {path} 失败")),
    };

    // 命令行给了模式就覆盖配置文件里的：CLI 优先级高于配置文件
    if let Some(raw) = std::env::args().nth(2) {
        config.mode = parse_mode(&raw).context("命令行给的运行模式无法识别")?;
    }

    if let Some(level) = config.log_level {
        telemetry
            .set_level(&level.to_string())
            .context("设置日志级别失败")?;
    }

    info!(?config, "配置已加载");
    print_line(&format!(
        "port = {}, mode = {}, log_level = {:?}",
        config.port, config.mode, config.log_level,
    ))
    .context("写入 stdout 失败")?;

    Ok(())
}

/// 把一行结果写到 stdout。
///
/// 下游管道提前关闭（`BrokenPipe`）时视为正常结束，其余写失败照常返回错误。
fn print_line(line: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    // 写入后立即 flush，让写失败在这里就返回
    match writeln!(out, "{line}").and_then(|()| out.flush()) {
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}
