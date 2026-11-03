//! 统一错误模型。
//!
//! [`AppError`] 经 IPC 序列化为 `{ "kind": "...", "message": "..." }` 判别联合，
//! 前端可按 `kind` 做类型化分支处理（开发文档 3 - 错误建模）。

use serde::Serialize;

/// 应用统一错误。
#[derive(Debug, thiserror::Error, Serialize, specta::Type)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum AppError {
    /// 连接配置读写失败。
    #[error("配置读写失败：{0}")]
    Config(String),

    /// 操作系统凭据存储访问失败。
    #[error("密钥存储访问失败：{0}")]
    Keyring(String),

    /// S3 请求失败（鉴权、权限、后端错误等）。
    #[error("S3 请求失败：{0}")]
    S3(String),

    /// 网络层失败或超时。
    #[error("网络请求失败：{0}")]
    Network(String),

    /// 调用方输入不合法。
    #[error("输入不合法：{0}")]
    Invalid(String),

    /// 目标资源不存在。
    #[error("资源不存在：{0}")]
    NotFound(String),
}

/// 统一结果类型。
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_shape_is_discriminated_union() {
        let err = AppError::S3("boom".into());
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "s3");
        assert_eq!(json["message"], "boom");
    }
}
