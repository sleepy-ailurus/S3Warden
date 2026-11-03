//! 密钥存储：操作系统凭据存储的唯一出入口。
//!
//! - Windows：凭据管理器；macOS：Keychain；Linux：Secret Service。
//! - Secret Key 只在本模块与 [`crate::api`] 之间流转，**绝不进入配置文件，绝不回传前端**（开发文档 6.4）。

use keyring::v1::{Entry, Error as KeyringError};

use crate::error::{AppError, AppResult};
use crate::types::{mask_access_key, SecretStatus};

/// 凭据存储中的服务名。
const SERVICE: &str = "dev.s3warden.connection";

fn entry(id: &str) -> AppResult<Entry> {
    Entry::new(SERVICE, &format!("connection/{id}/secret"))
        .map_err(|e| AppError::Keyring(format!("打开系统凭据存储失败：{e}")))
}

/// 保存（覆盖）某连接的 Secret Key。
pub fn save_secret(id: &str, secret: &str) -> AppResult<()> {
    entry(id)?
        .set_password(secret)
        .map_err(|e| AppError::Keyring(format!("保存密钥失败：{e}")))
}

/// 读取某连接的 Secret Key；未配置返回 `None`。
pub fn load_secret(id: &str) -> AppResult<Option<String>> {
    match entry(id)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(format!("读取密钥失败：{e}"))),
    }
}

/// 删除某连接的 Secret Key；条目不存在视为成功（保证幂等）。
pub fn delete_secret(id: &str) -> AppResult<()> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(format!("删除密钥失败：{e}"))),
    }
}

/// 查询密钥状态（脱敏信息，可回传前端）。
pub fn secret_status(id: &str, access_key: &str) -> SecretStatus {
    SecretStatus {
        configured: load_secret(id).unwrap_or(None).is_some(),
        access_key_masked: mask_access_key(access_key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 依赖操作系统凭据存储，仅在 Windows 上执行；其他平台跳过。
    #[test]
    #[cfg(windows)]
    fn keyring_roundtrip() {
        let id = format!("test-{}", uuid::Uuid::new_v4());
        assert!(load_secret(&id).unwrap().is_none());

        save_secret(&id, "topsecret").unwrap();
        assert_eq!(load_secret(&id).unwrap().as_deref(), Some("topsecret"));

        delete_secret(&id).unwrap();
        assert!(load_secret(&id).unwrap().is_none());

        // 幂等删除
        delete_secret(&id).unwrap();
    }
}
