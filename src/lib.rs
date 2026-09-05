//! RUST ECOSYSTEM
//!
//! 库目标：存放业务逻辑，供 `src/main.rs` 与 `tests/` 调用。

mod error;

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use tokio::fs;

pub use crate::error::{Error, Result};

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

    /// 端口解析出来是 0
    #[error("端口不能为 0")]
    ZeroPort,

    /// 内容不是合法的 `u16`
    #[error("端口不是合法数字")]
    InvalidPort(#[from] std::num::ParseIntError),
}

/// 读配置文件并解析出端口。
///
/// # Errors
///
/// 文件不存在返回 [`ConfigError::NotFound`]，其余 IO 失败返回 [`ConfigError::Io`]，
/// 内容非法见 [`parse_port`]。
pub async fn load_port(path: impl AsRef<Path>) -> Result<u16, ConfigError> {
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

    parse_port(&text)
}

/// 从配置内容里解析端口。纯函数，不碰文件系统。
///
/// # Errors
///
/// 不是合法数字返回 [`ConfigError::InvalidPort`]，解析出 0 返回 [`ConfigError::ZeroPort`]。
pub fn parse_port(text: &str) -> Result<u16, ConfigError> {
    let port = text.trim().parse::<u16>()?;
    if port == 0 {
        return Err(ConfigError::ZeroPort);
    }
    Ok(port)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{ConfigError, load_port, parse_port};

    // ---------- 纯函数：同步、不落盘、毫秒级 ----------

    #[test]
    fn parses_a_valid_port() {
        assert_eq!(parse_port("8080\n").unwrap(), 8080); // 顺便验证 trim() 生效
        assert_eq!(parse_port("  443  ").unwrap(), 443);
    }

    #[test]
    fn rejects_non_numeric() {
        assert!(matches!(
            parse_port("abc"),
            Err(ConfigError::InvalidPort(_))
        ));
    }

    #[test]
    fn rejects_zero() {
        assert!(matches!(parse_port("0"), Err(ConfigError::ZeroPort)));
    }

    /// 「不是数字」和「超出 u16 范围」落在同一个变体里，
    /// 但底层 `ParseIntError` 的 kind 不同 —— 这就是当初不把它 `to_string()`
    /// 拍扁的回报：信息还在，调用方随时能取。
    #[test]
    fn overflow_and_garbage_are_distinguishable() {
        use std::num::IntErrorKind;

        let Err(ConfigError::InvalidPort(err)) = parse_port("70000") else {
            panic!("70000 超出 u16，应该是 InvalidPort");
        };
        assert_eq!(err.kind(), &IntErrorKind::PosOverflow);

        let Err(ConfigError::InvalidPort(err)) = parse_port("abc") else {
            panic!("abc 不是数字，应该是 InvalidPort");
        };
        assert_eq!(err.kind(), &IntErrorKind::InvalidDigit);
    }

    // ---------- IO：只剩这几个需要真的碰文件系统 ----------

    /// nextest 让每个测试跑在独立进程里、并行执行，
    /// 所以每个测试用自己的文件名，不要共用。
    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ecosystem-test-{tag}.toml"))
    }

    #[tokio::test]
    async fn loads_port_from_file() {
        let path = temp_path("ok");
        tokio::fs::write(&path, "8080\n").await.unwrap();

        // &PathBuf 满足 AsRef<Path>，不用再 .to_str().unwrap()
        let port = load_port(&path).await.unwrap();
        assert_eq!(port, 8080);

        tokio::fs::remove_file(&path).await.unwrap();
    }

    #[tokio::test]
    async fn missing_file_reports_not_found() {
        let path = temp_path("missing"); // 只造名字，不写文件
        let err = load_port(&path).await.unwrap_err();

        assert!(matches!(err, ConfigError::NotFound { .. }));
        // NotFound 是我们自己造的语义错误，没有底层原因
        assert!(std::error::Error::source(&err).is_none());
    }

    #[tokio::test]
    async fn directory_reports_io_error() {
        // 把一个目录当配置文件读，系统返回 IsADirectory，落到 Io 变体
        let err = load_port(std::env::temp_dir()).await.unwrap_err();
        assert!(matches!(err, ConfigError::Io { .. }));
    }

    /// 验证错误链真的接上了：我们补的上下文在最外层，底层原因还挂在 source 上。
    #[tokio::test]
    async fn io_error_keeps_its_source() {
        use std::error::Error as _;

        let err = load_port(std::env::temp_dir()).await.unwrap_err();

        // 最外层是我们写的消息，带上了路径
        assert!(err.to_string().contains("读取配置文件"));

        // 底层那个 io::Error 还在链上，可以原样取回来
        let source = err.source().expect("Io 变体应该带 source");
        assert!(source.downcast_ref::<std::io::Error>().is_some());
    }
}
