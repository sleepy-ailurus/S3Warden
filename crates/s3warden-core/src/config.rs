//! 连接配置存储。
//!
//! 非密钥配置存放在用户配置目录的 `connections.json`；
//! **Secret Key 永远不落此文件**，只存于操作系统凭据存储（见 [`crate::secrets`]）。
//! 写入采用「临时文件 + 原子替换」，避免写一半损坏配置。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::types::ConnectionProfile;

/// 连接档案存储。
pub struct ProfileStore {
    path: PathBuf,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ProfileFile {
    profiles: Vec<ConnectionProfile>,
}

impl ProfileStore {
    /// 打开用户配置目录下的存储（`%APPDATA%/s3warden` / `~/.config/s3warden` 等）。
    pub fn open() -> AppResult<Self> {
        let dir = dirs::config_dir()
            .ok_or_else(|| AppError::Config("无法定位用户配置目录".to_string()))?
            .join("s3warden");
        Self::at_dir(&dir)
    }

    /// 在指定目录打开存储（供测试注入）。
    pub fn at_dir(dir: &Path) -> AppResult<Self> {
        fs::create_dir_all(dir)
            .map_err(|e| AppError::Config(format!("创建配置目录失败：{e}")))?;
        Ok(Self {
            path: dir.join("connections.json"),
        })
    }

    fn load(&self) -> AppResult<ProfileFile> {
        if !self.path.exists() {
            return Ok(ProfileFile::default());
        }
        let raw = fs::read_to_string(&self.path)
            .map_err(|e| AppError::Config(format!("读取配置失败：{e}")))?;
        serde_json::from_str(&raw).map_err(|e| AppError::Config(format!("解析配置失败：{e}")))
    }

    fn save(&self, file: &ProfileFile) -> AppResult<()> {
        let tmp = self.path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(file)
            .map_err(|e| AppError::Config(format!("序列化配置失败：{e}")))?;
        fs::write(&tmp, raw).map_err(|e| AppError::Config(format!("写入配置失败：{e}")))?;
        fs::rename(&tmp, &self.path)
            .map_err(|e| AppError::Config(format!("替换配置文件失败：{e}")))?;
        Ok(())
    }

    /// 列出全部连接（按名称排序）。
    pub fn list(&self) -> AppResult<Vec<ConnectionProfile>> {
        let mut profiles = self.load()?.profiles;
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    /// 读取单个连接。
    pub fn get(&self, id: &str) -> AppResult<ConnectionProfile> {
        self.load()?
            .profiles
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::NotFound(format!("连接 {id} 不存在")))
    }

    /// 新增或整体替换一个连接档案。
    pub fn upsert(&self, profile: ConnectionProfile) -> AppResult<()> {
        let mut file = self.load()?;
        match file.profiles.iter().position(|p| p.id == profile.id) {
            Some(i) => file.profiles[i] = profile,
            None => file.profiles.push(profile),
        }
        self.save(&file)
    }

    /// 删除连接档案。
    pub fn remove(&self, id: &str) -> AppResult<()> {
        let mut file = self.load()?;
        let before = file.profiles.len();
        file.profiles.retain(|p| p.id != id);
        if file.profiles.len() == before {
            return Err(AppError::NotFound(format!("连接 {id} 不存在")));
        }
        self.save(&file)
    }

    /// 设置默认连接（同时取消其他连接的默认标记）。
    pub fn set_default(&self, id: &str) -> AppResult<()> {
        let mut file = self.load()?;
        let mut found = false;
        for p in file.profiles.iter_mut() {
            if p.id == id {
                p.is_default = true;
                found = true;
            } else {
                p.is_default = false;
            }
        }
        if !found {
            return Err(AppError::NotFound(format!("连接 {id} 不存在")));
        }
        self.save(&file)
    }
}

/// 当前时间的 RFC3339 表示（UTC）。
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BackendKind, BackendPreset};

    fn temp_store() -> ProfileStore {
        let dir = std::env::temp_dir().join(format!("s3warden-test-{}", uuid::Uuid::new_v4()));
        ProfileStore::at_dir(&dir).expect("创建临时存储失败")
    }

    fn sample(id: &str, name: &str) -> ConnectionProfile {
        ConnectionProfile {
            id: id.to_string(),
            name: name.to_string(),
            endpoint: "http://127.0.0.1:9000".to_string(),
            region: "us-east-1".to_string(),
            access_key: "minioadmin".to_string(),
            path_style: true,
            preset: BackendPreset::Minio,
            backend: Some(BackendKind::Minio),
            is_default: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn upsert_list_remove_roundtrip() {
        let store = temp_store();
        store.upsert(sample("a", "本地 MinIO")).unwrap();
        store.upsert(sample("b", "RustFS")).unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);

        // 更新走 upsert 同一 id
        let mut updated = sample("a", "改名");
        updated.endpoint = "http://127.0.0.1:9001".to_string();
        store.upsert(updated).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
        assert_eq!(store.get("a").unwrap().endpoint, "http://127.0.0.1:9001");

        store.remove("a").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(store.get("a").is_err());
    }

    #[test]
    fn set_default_is_exclusive() {
        let store = temp_store();
        store.upsert(sample("a", "A")).unwrap();
        store.upsert(sample("b", "B")).unwrap();

        store.set_default("b").unwrap();
        assert!(!store.get("a").unwrap().is_default);
        assert!(store.get("b").unwrap().is_default);

        store.set_default("a").unwrap();
        assert!(store.get("a").unwrap().is_default);
        assert!(!store.get("b").unwrap().is_default);
    }

    #[test]
    fn remove_missing_returns_not_found() {
        let store = temp_store();
        let err = store.remove("nope").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }
}
