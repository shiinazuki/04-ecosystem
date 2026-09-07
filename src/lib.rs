//! RUST ECOSYSTEM
//!
//! 库目标：存放业务逻辑，供 `src/main.rs` 与 `tests/` 调用。

mod error;

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use strum::VariantNames as _;
use tokio::fs;

pub use crate::error::{Error, Result};

/// 应用配置，由 TOML 配置文件反序列化而来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// 监听端口，不能为 0。
    #[serde(deserialize_with = "non_zero_port")]
    pub port: u16,
    /// 运行模式。
    pub mode: Mode,

    /// 日志级别
    pub log_level: Option<LogLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// 运行模式。
///
/// `strum` 负责命令行那一侧的字符串互转，`serde` 负责配置文件那一侧。
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    strum::EnumIter,
    strum::VariantNames,
    Serialize,
    Deserialize,
)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
#[serde(try_from = "String", into = "String")]
pub enum Mode {
    /// 本地开发。
    Dev,
    /// 预发布。
    Staging,
    /// 生产。
    Prod,
    /// 自动化测试。
    Test,
}

impl TryFrom<String> for Mode {
    type Error = strum::ParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Mode> for String {
    fn from(value: Mode) -> Self {
        value.to_string()
    }
}

/// 加载端口配置时可能出现的错误。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// 配置文件不存在。
    #[error("找不到配置文件: {}", path.display())]
    NotFound {
        /// 用户给出的路径
        path: PathBuf,
    },

    /// 读取配置文件时的其他 IO 失败（权限、是个目录、磁盘错误……）
    #[error("读取配置文件 {} 失败", path.display())]
    Io {
        /// 出错的路径
        path: PathBuf,

        /// 底层 IO 错误。字段名叫 `source`，thiserror 就会自动把它接进错误链
        source: std::io::Error,
    },

    /// 命令行传进来的模式名不认识。
    #[error("未知的运行模式 {value}，可选值：{}", Mode::VARIANTS.join(" / "))]
    UnknownMode { value: String },

    /// 配置文件不是合法 TOML，或者字段类型 / 取值对不上。
    #[error("配置文件格式错误")]
    Toml(#[from] toml::de::Error),
}

/// 从一段字符串解析运行模式，供命令行参数使用。
///
/// 配置文件那一侧走 serde，不经过这里。
///
/// # Errors
///
/// 不是合法的模式名时返回 [`ConfigError::UnknownMode`]，消息里会列出全部合法值。
pub fn parse_mode(raw: &str) -> Result<Mode, ConfigError> {
    let raw = raw.trim();
    raw.parse::<Mode>().map_err(|_| ConfigError::UnknownMode {
        value: raw.to_owned(),
    })
}

/// 把一段 TOML 文本解析成 [`Config`]，并做业务校验。  ← 校验已经搬进 serde 了
pub fn parse_config(text: &str) -> Result<Config, ConfigError> {
    Ok(toml::from_str::<Config>(text)?)
}

/// 读配置文件并解析成 [`Config`]。
///
/// IO 只在这一层，解析逻辑全在 [`parse_config`] 里。
///
/// # Errors
///
/// 文件不存在返回 [`ConfigError::NotFound`]，其余读取失败返回 [`ConfigError::Io`]，
/// 内容非法见 [`parse_config`]。
pub async fn load_config(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
    let path = path.as_ref();

    let text = fs::read_to_string(path)
        .await
        .map_err(|source| match source.kind() {
            ErrorKind::NotFound => ConfigError::NotFound {
                path: path.to_path_buf(),
            },
            _ => ConfigError::Io {
                path: path.to_path_buf(),
                source,
            },
        })?;

    parse_config(&text)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 8080,
            mode: Mode::Dev,
            log_level: None,
        }
    }
}

fn non_zero_port<'de, D>(d: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    let port = u16::deserialize(d)?;
    if port == 0 {
        return Err(D::Error::custom("端口不能为 0"));
    }
    Ok(port)
}

#[cfg(test)]
mod tests {
    use std::{error::Error as _, path::PathBuf};

    use strum::IntoEnumIterator as _;

    use super::{Config, ConfigError, Mode, load_config, parse_config, parse_mode};

    /// 造一段最小可用的 TOML。
    fn toml_of(port: &str, mode: &str) -> String {
        format!("port = {port}\nmode = \"{mode}\"\n")
    }

    /// 取错误链的下一层，用来断言底层原因还在。
    fn source_of(err: &ConfigError) -> String {
        err.source().expect("这个变体应该带 source").to_string()
    }

    // ---------- parse_config：纯函数，不落盘 ----------

    #[test]
    fn parses_a_valid_config() {
        let config = parse_config(&toml_of("8080", "prod")).unwrap();
        assert_eq!(
            config,
            Config {
                port: 8080,
                mode: Mode::Prod,
                log_level: None,
            }
        );
    }

    #[test]
    fn comments_and_blank_lines_are_fine() {
        let text = "# 服务端口\nport = 443\n\nmode = \"dev\"  # 本地开发\n";
        assert_eq!(parse_config(text).unwrap().port, 443);
    }

    #[test]
    fn missing_field_is_a_toml_error() {
        let err = parse_config("port = 8080\n").unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
        assert!(source_of(&err).contains("missing field"));
    }

    #[test]
    fn port_out_of_u16_range_is_a_toml_error() {
        // 70000 超出 u16，serde 在反序列化阶段就拦下了，轮不到 ZeroPort
        let err = parse_config(&toml_of("70000", "prod")).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
    }

    #[test]
    fn not_even_toml() {
        let err = parse_config("这不是 TOML {{{").unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
    }

    // ---------- Mode：strum 与 serde 两侧 ----------

    #[test]
    fn mode_roundtrips_through_strum() {
        for mode in Mode::iter() {
            assert_eq!(mode.to_string().parse::<Mode>().unwrap(), mode);
        }
    }

    #[test]
    fn parse_mode_trims_and_ignores_case() {
        assert_eq!(parse_mode("  STAGING ").unwrap(), Mode::Staging);
    }

    #[test]
    fn parse_mode_error_lists_every_variant() {
        let msg = parse_mode("nope").unwrap_err().to_string();
        for mode in Mode::iter() {
            assert!(msg.contains(&mode.to_string()), "缺少变体 {mode}");
        }
    }

    /// ⭐ `#[strum(serialize_all)]` 和 `#[serde(rename_all)]` 是两套独立的标注，
    /// 谁改漏了都不会报错。这个测试把「它们必须一致」变成可检测的。
    #[test]
    fn strum_and_serde_agree_on_every_name() {
        for mode in Mode::iter() {
            let text = toml_of("8080", &mode.to_string());
            assert_eq!(
                parse_config(&text).unwrap().mode,
                mode,
                "strum 打印成 {mode}，但 serde 不认这个名字"
            );
        }
    }

    // ---------- load_config：只有这几个需要碰文件系统 ----------

    /// nextest 让每个测试跑在独立进程里、并行执行，所以文件名不能共用。
    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecosystem-test-{tag}.toml"))
    }

    #[tokio::test]
    async fn loads_config_from_a_real_file() {
        let path = temp_path("ok");
        tokio::fs::write(&path, toml_of("8080", "staging"))
            .await
            .unwrap();

        let config = load_config(&path).await.unwrap();
        assert_eq!(config.mode, Mode::Staging);

        tokio::fs::remove_file(&path).await.unwrap();
    }

    #[tokio::test]
    async fn missing_file_reports_not_found() {
        let path = temp_path("missing"); // 只造名字，不写文件
        let err = load_config(&path).await.unwrap_err();

        assert!(matches!(err, ConfigError::NotFound { .. }));
        // NotFound 是我们自己造的语义错误，没有底层原因
        assert!(err.source().is_none());
    }

    #[tokio::test]
    async fn directory_reports_io_error() {
        // 把目录当配置文件读，系统返回 IsADirectory，落到 Io 变体
        let err = load_config(std::env::temp_dir()).await.unwrap_err();
        assert!(matches!(err, ConfigError::Io { .. }));
    }

    #[tokio::test]
    async fn io_error_keeps_its_source() {
        let err = load_config(std::env::temp_dir()).await.unwrap_err();

        // 最外层是我们写的消息，带上了路径
        assert!(err.to_string().contains("读取配置文件"));

        // 底层那个 io::Error 还在链上，可以原样取回来
        let source = err.source().expect("Io 变体应该带 source");
        assert!(source.downcast_ref::<std::io::Error>().is_some());
    }

    #[test]
    fn typo_in_optional_field_is_rejected() {
        // 少个下划线。没有 deny_unknown_fields 的话，这会被静默忽略
        let err = parse_config("port = 8080\nmode = \"dev\"\nloglevel = \"debug\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
        assert!(source_of(&err).contains("unknown field"));
    }

    #[test]
    fn unknown_mode_is_rejected() {
        let err = parse_config(&toml_of("8080", "nope")).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
        // try_from 之后错误来自 strum
        assert!(source_of(&err).contains("Matching variant not found"));
    }

    #[test]
    fn zero_port_is_rejected_by_serde() {
        let err = parse_config(&toml_of("0", "prod")).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
        // deserialize_with 的好处：错误带行列定位
        assert!(source_of(&err).contains("端口不能为 0"));
    }
}
