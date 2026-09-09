//! 日志与追踪的初始化。
//!
//! 两层输出，各管各的：
//!
//! | 层 | 去向 | 级别 | 给谁看 |
//! | --- | --- | --- | --- |
//! | console | stderr | 可运行时调整 | 人 |
//! | file | 按天滚动的 JSON | 固定 `INFO` | 日志系统 |
//!
//! 环境变量：
//!
//! - `RUST_LOG`：过滤指令，优先于代码里给的默认级别
//! - `ECOSYSTEM_LOG_DIR`：日志目录；**设成空字符串就完全不写文件**（容器里通常只要 stdout）
//! - `NO_COLOR`：非空时关掉终端颜色

use std::io::IsTerminal as _;

use anyhow::Context as _;
use tracing_appender::{non_blocking::WorkerGuard, rolling::RollingFileAppender};
use tracing_subscriber::{
    EnvFilter, Layer, Registry, filter::LevelFilter, fmt, fmt::format::FmtSpan,
    layer::SubscriberExt, reload, util::SubscriberInitExt,
};

/// 指定日志目录的环境变量。
const LOG_DIR_ENV: &str = "ECOSYSTEM_LOG_DIR";
/// 日志目录的默认值。
const DEFAULT_LOG_DIR: &str = "logs";
/// 滚动日志最多保留几份，超出的自动删除 —— 不加这个磁盘迟早满。
const MAX_LOG_FILES: usize = 7;
/// 文件层固定记 `INFO` 及以上：文件是给运维查的，不该被临时调试级别刷爆。
const FILE_LEVEL: LevelFilter = LevelFilter::INFO;

/// 全局 subscriber 装好之后的句柄。
///
/// **必须持有到进程结束**：里面的 [`WorkerGuard`] 一旦 drop，写文件的后台线程就收摊，
/// 缓冲区里还没落盘的日志会全部丢失。
#[derive(Debug)]
pub(crate) struct Telemetry {
    reload: reload::Handle<EnvFilter, Registry>,
    _guard: Option<WorkerGuard>,
    env_override: bool,
}

impl Telemetry {
    /// 运行时调整**终端**那一层的过滤级别（文件层不受影响）。
    ///
    /// # Errors
    ///
    /// `directives` 不是合法的 `EnvFilter` 指令，或全局 subscriber 已经没了。
    pub(crate) fn set_level(&self, directives: &str) -> anyhow::Result<()> {
        if self.env_override {
            tracing::debug!(directives, "RUST_LOG 已显式设置，忽略配置文件里的日志级别");
            return Ok(());
        }
        let filter = EnvFilter::try_new(directives)
            .with_context(|| format!("无法解析日志过滤指令 `{directives}`"))?;
        self.reload
            .reload(filter)
            .context("重新加载日志过滤器失败")?;
        tracing::debug!(directives, "终端日志级别已切换");

        Ok(())
    }
}

/// 安装全局 subscriber，在 `main` 最开头调用一次。
///
/// # Errors
///
/// 日志目录建不出来，或全局 subscriber 已经被别人装过了。
pub(crate) fn init(default_level: &str) -> anyhow::Result<Telemetry> {
    // `default_level` 解析不了时退回 info
    let default = default_level.parse().unwrap_or_else(|_| {
        eprintln!("无法解析日志级别 `{default_level}`，退回 info");
        tracing::Level::INFO.into()
    });

    warn_if_env_looks_like_a_typo();

    let env_override =
        std::env::var(EnvFilter::DEFAULT_ENV).is_ok_and(|value| !value.trim().is_empty());

    // RUST_LOG 优先；它为空或没设时才用 default
    let env_filter = EnvFilter::builder()
        .with_default_directive(default)
        .with_env_var(EnvFilter::DEFAULT_ENV)
        .from_env_lossy();
    let (console_filter, reload) = reload::Layer::new(env_filter);

    // 终端层：给人看。FmtSpan::CLOSE 让每个 span 结束时打一行，自带耗时
    let console = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(ansi_enabled())
        .with_target(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(console_filter);

    // 文件层：给机器看。JSON + 完整 span 上下文，方便日志系统按字段检索
    let (file, guard) = match log_dir() {
        None => (None, None),
        Some(dir) => {
            // 目录不存在时 tracing-appender 的清理逻辑会往 stderr 吐一行报错，先自己建好。
            #[expect(
                clippy::disallowed_methods,
                reason = "日志初始化发生在 runtime 启动之前，无法使用 tokio::fs"
            )]
            std::fs::create_dir_all(&dir).with_context(|| format!("无法创建日志目录 {dir}"))?;

            let appender = RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("ecosystem")
                .filename_suffix("log")
                .max_log_files(MAX_LOG_FILES)
                .build(&dir)
                .with_context(|| format!("无法在 {dir} 建立日志文件"))?;
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let layer = fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_filter(FILE_LEVEL);
            (Some(layer), Some(guard))
        }
    };

    Registry::default()
        .with(console)
        .with(file)
        .try_init()
        .context("全局 subscriber 已经被装过了，init 只能调用一次")?;

    install_panic_hook();

    Ok(Telemetry {
        reload,
        _guard: guard,
        env_override,
    })
}

/// 判断日志是否上色：stderr 是终端、且未设置非空的 `NO_COLOR` 时返回 `true`。
fn ansi_enabled() -> bool {
    let disabled_by_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    !disabled_by_env && std::io::stderr().is_terminal()
}

/// 日志目录：环境变量优先，设成空字符串表示不写文件。
fn log_dir() -> Option<String> {
    match std::env::var(LOG_DIR_ENV) {
        Ok(dir) if dir.trim().is_empty() => None,
        Ok(dir) => Some(dir),
        Err(_) => Some(DEFAULT_LOG_DIR.to_owned()),
    }
}

/// 让 panic 也进日志系统 —— 默认的 panic 只写 stderr，不会进 JSON 文件。
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "thread panic");
        previous(info);
    }));
}

/// `RUST_LOG` 被写成单个裸词、且它不是日志级别时，打印一行提示。
///
/// 只提示，不改变过滤行为。
fn warn_if_env_looks_like_a_typo() {
    let Ok(raw) = std::env::var(EnvFilter::DEFAULT_ENV) else {
        return;
    };
    let raw = raw.trim();

    // 空值和带 `=` / `,` 的按模块细分写法交给 EnvFilter 自己判断
    if raw.is_empty() || raw.contains('=') || raw.contains(',') {
        return;
    }
    // 能解析成级别（名字、`off`、0-5 的数字）即为正常用法
    if raw.parse::<LevelFilter>().is_ok() {
        return;
    }

    eprintln!("提示：RUST_LOG=`{raw}` 不是日志级别，按 EnvFilter 的语法它是一个「目标名」，");
    eprintln!("      即只放行名为 {raw} 的模块。想调级别请写 error / warn / info / debug / trace");
}
