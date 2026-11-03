//! 面向 Tauri 壳的编排层：组合配置存储、密钥存储、探测与 S3 操作。
//!
//! 安全约束（开发文档 6.4）：
//! - 返回值中永不包含 Secret Key；
//! - Secret 仅在创建/更新/测试链路中短暂持有。

use std::path::Path;
use std::time::Instant;

use uuid::Uuid;

use crate::config::{now_rfc3339, ProfileStore};
use crate::error::{AppError, AppResult};
use crate::types::{
    BackendPreset, BackendPresetInfo, BucketDeleteResult, BucketPolicyInfo, ConnectionInput,
    ConnectionPatch,
    ConnectionProfile, ConnectionTestOk, DownloadResult, ObjectDetail, ObjectEntry, ObjectPage,
    ObjectTag, PresignedUrl, RenameDirResult, SecretStatus, TextPreview, UploadSummary,
};
use crate::{probe, s3, secrets};

fn validate(
    name: &str,
    endpoint: &str,
    access_key: &str,
    secret_key: Option<&str>,
) -> AppResult<()> {
    if name.trim().is_empty() {
        return Err(AppError::Invalid("连接名称不能为空".to_string()));
    }
    let ep = endpoint.trim();
    if !(ep.starts_with("http://") || ep.starts_with("https://")) {
        return Err(AppError::Invalid(
            "端点必须以 http:// 或 https:// 开头".to_string(),
        ));
    }
    if access_key.trim().is_empty() {
        return Err(AppError::Invalid("Access Key 不能为空".to_string()));
    }
    if let Some(sk) = secret_key {
        if sk.is_empty() {
            return Err(AppError::Invalid("Secret Key 不能为空".to_string()));
        }
    }
    Ok(())
}

fn normalize_region(region: Option<&str>) -> String {
    region
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .unwrap_or(s3::DEFAULT_REGION)
        .to_string()
}

/// 显式读取某连接的 Secret Key 明文。
///
/// 只在用户主动打开「编辑连接」时调用，用于回填密钥输入框；
/// 其余链路（连接列表、详情、测试）一律不回传明文（开发文档 6.4）。
pub fn reveal_secret(id: &str) -> AppResult<String> {
    let _ = get_connection(id)?; // 确保连接存在
    secrets::load_secret(id)?.ok_or_else(|| {
        AppError::NotFound("系统凭据存储中没有该连接的密钥".to_string())
    })
}

/// 创建连接：探测后端 → 写密钥 → 落配置。
///
/// 顺序刻意为之：先写密钥再落配置，避免出现「有连接档案却无密钥」的悬空状态。
pub async fn create_connection(input: ConnectionInput) -> AppResult<ConnectionProfile> {
    validate(&input.name, &input.endpoint, &input.access_key, Some(&input.secret_key))?;

    let store = ProfileStore::open()?;
    let (backend, _) = probe::probe_backend(input.endpoint.trim()).await;
    let preset = input.preset.unwrap_or_default();

    let is_default = store.list()?.is_empty();
    let profile = ConnectionProfile {
        id: Uuid::new_v4().to_string(),
        name: input.name.trim().to_string(),
        endpoint: input.endpoint.trim().trim_end_matches('/').to_string(),
        region: normalize_region(input.region.as_deref()),
        access_key: input.access_key.trim().to_string(),
        path_style: input
            .path_style
            .unwrap_or_else(|| preset.default_path_style()),
        preset,
        backend: Some(backend),
        is_default,
        created_at: now_rfc3339(),
        updated_at: now_rfc3339(),
    };

    secrets::save_secret(&profile.id, &input.secret_key)?;
    store.upsert(profile.clone())?;
    Ok(profile)
}

/// 列出可选的后端接入类型及其默认参数（新建连接表单的数据源）。
pub fn list_backend_presets() -> Vec<BackendPresetInfo> {
    BackendPreset::all()
        .into_iter()
        .map(|p| BackendPresetInfo {
            preset: p,
            default_path_style: p.default_path_style(),
            endpoint_hint: p.endpoint_hint().to_string(),
            region_hint: p.region_hint().to_string(),
            notes: p.notes().to_string(),
        })
        .collect()
}

/// 列出全部连接。
pub fn list_connections() -> AppResult<Vec<ConnectionProfile>> {
    ProfileStore::open()?.list()
}

/// 读取单个连接。
pub fn get_connection(id: &str) -> AppResult<ConnectionProfile> {
    ProfileStore::open()?.get(id)
}

/// 更新连接。`patch.secret_key` 为 None 或空串表示不修改密钥。
pub fn update_connection(id: &str, patch: ConnectionPatch) -> AppResult<ConnectionProfile> {
    let store = ProfileStore::open()?;
    let mut profile = store.get(id)?;

    if let Some(name) = &patch.name {
        if name.trim().is_empty() {
            return Err(AppError::Invalid("连接名称不能为空".to_string()));
        }
        profile.name = name.trim().to_string();
    }
    if let Some(endpoint) = &patch.endpoint {
        let ep = endpoint.trim();
        if !(ep.starts_with("http://") || ep.starts_with("https://")) {
            return Err(AppError::Invalid(
                "端点必须以 http:// 或 https:// 开头".to_string(),
            ));
        }
        profile.endpoint = ep.trim_end_matches('/').to_string();
    }
    if let Some(region) = &patch.region {
        profile.region = normalize_region(Some(region));
    }
    if let Some(ak) = &patch.access_key {
        if ak.trim().is_empty() {
            return Err(AppError::Invalid("Access Key 不能为空".to_string()));
        }
        profile.access_key = ak.trim().to_string();
    }
    if let Some(preset) = patch.preset {
        profile.preset = preset;
    }
    if let Some(path_style) = patch.path_style {
        profile.path_style = path_style;
    }
    if let Some(sk) = &patch.secret_key {
        if !sk.is_empty() {
            secrets::save_secret(id, sk)?;
        }
    }

    profile.updated_at = now_rfc3339();
    store.upsert(profile.clone())?;
    Ok(profile)
}

/// 删除连接：先删档案再清密钥；密钥删除幂等，残留可重试清理。
pub fn delete_connection(id: &str) -> AppResult<()> {
    let store = ProfileStore::open()?;
    store.remove(id)?;
    secrets::delete_secret(id)?;
    Ok(())
}

/// 设置默认连接。
pub fn set_default_connection(id: &str) -> AppResult<()> {
    ProfileStore::open()?.set_default(id)
}

/// 查询密钥状态（脱敏）。
pub fn secret_status(id: &str) -> AppResult<SecretStatus> {
    let profile = get_connection(id)?;
    Ok(secrets::secret_status(id, &profile.access_key))
}

async fn run_test(
    endpoint: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
    path_style: bool,
) -> AppResult<ConnectionTestOk> {
    let started = Instant::now();
    let client = s3::build_client(endpoint, region, access_key, secret_key, path_style);
    let buckets = s3::list_buckets(&client).await?;
    let latency_ms = started.elapsed().as_millis() as u32;

    let (backend, server_header) = probe::probe_backend(endpoint).await;
    Ok(ConnectionTestOk {
        latency_ms,
        backend,
        server_header,
        bucket_count: buckets.len() as u32,
    })
}

/// 测试未保存的连接（新建对话框场景）。
pub async fn test_new_connection(input: ConnectionInput) -> AppResult<ConnectionTestOk> {
    validate(&input.name, &input.endpoint, &input.access_key, Some(&input.secret_key))?;
    run_test(
        input.endpoint.trim(),
        &normalize_region(input.region.as_deref()),
        input.access_key.trim(),
        &input.secret_key,
        input
            .path_style
            .unwrap_or_else(|| input.preset.unwrap_or_default().default_path_style()),
    )
    .await
}

/// 测试已保存的连接。
pub async fn test_saved_connection(id: &str) -> AppResult<ConnectionTestOk> {
    let profile = get_connection(id)?;
    let secret = secrets::load_secret(id)?
        .ok_or_else(|| AppError::Keyring("该连接尚未配置 Secret Key".to_string()))?;
    run_test(
        &profile.endpoint,
        &profile.region,
        &profile.access_key,
        &secret,
        profile.path_style,
    )
    .await
}

async fn client_for(profile: &ConnectionProfile) -> AppResult<s3::Client> {
    let secret = secrets::load_secret(&profile.id)?
        .ok_or_else(|| AppError::Keyring("该连接尚未配置 Secret Key".to_string()))?;
    Ok(s3::build_client(
        &profile.endpoint,
        &profile.region,
        &profile.access_key,
        &secret,
        profile.path_style,
    ))
}

/// 列出某连接的全部桶。
pub async fn list_buckets(id: &str) -> AppResult<Vec<crate::types::BucketSummary>> {
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::list_buckets(&client).await
}

/// 校验桶名（S3 通用命名规则）。
///
/// 规则：3~63 字符；仅小写字母、数字、`.`、`-`；首尾必须是字母或数字；
/// 不能是 IP 地址形式；不允许连续的点。
pub fn validate_bucket_name(name: &str) -> AppResult<()> {
    let n = name.trim();
    if n.is_empty() {
        return Err(AppError::Invalid("桶名不能为空".to_string()));
    }
    if n.len() < 3 || n.len() > 63 {
        return Err(AppError::Invalid(
            "桶名长度必须为 3~63 个字符".to_string(),
        ));
    }
    if !n
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
    {
        return Err(AppError::Invalid(
            "桶名只能包含小写字母、数字、点（.）和连字符（-）".to_string(),
        ));
    }
    let first = n.chars().next().unwrap();
    let last = n.chars().last().unwrap();
    if !(first.is_ascii_lowercase() || first.is_ascii_digit())
        || !(last.is_ascii_lowercase() || last.is_ascii_digit())
    {
        return Err(AppError::Invalid(
            "桶名必须以小写字母或数字开头和结尾".to_string(),
        ));
    }
    if n.contains("..") {
        return Err(AppError::Invalid("桶名不能包含连续的点".to_string()));
    }
    if n.parse::<std::net::Ipv4Addr>().is_ok() {
        return Err(AppError::Invalid("桶名不能是 IP 地址形式".to_string()));
    }
    Ok(())
}

/// 在当前连接下创建桶（区域沿用连接档案配置）。
pub async fn create_bucket(id: &str, bucket: &str) -> AppResult<()> {
    validate_bucket_name(bucket)?;
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::create_bucket(&client, bucket.trim(), &profile.region).await
}

/// 删除桶。
///
/// `force = true` 时先批量清空桶内全部对象再删桶（用于非空桶的二次确认后强制删除）；
/// `force = false` 时直接删桶，非空桶会被服务端拒绝（BucketNotEmpty）。
pub async fn delete_bucket(id: &str, bucket: &str, force: bool) -> AppResult<BucketDeleteResult> {
    if bucket.trim().is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;

    if !force {
        s3::delete_bucket(&client, bucket).await?;
        return Ok(BucketDeleteResult { removed_objects: 0 });
    }

    let keys = s3::list_all_keys(&client, bucket, "").await?;
    let removed = s3::delete_objects(&client, bucket, &keys).await?;
    s3::delete_bucket(&client, bucket).await?;
    Ok(BucketDeleteResult {
        removed_objects: removed as u32,
    })
}

/// 按前缀浏览对象。
pub async fn list_objects(
    id: &str,
    bucket: &str,
    prefix: &str,
    token: Option<String>,
) -> AppResult<ObjectPage> {
    if bucket.is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::list_objects(&client, bucket, prefix, token.as_deref()).await
}

/// 读取对象元数据（详情面板）。
pub async fn stat_object(id: &str, bucket: &str, key: &str) -> AppResult<ObjectDetail> {
    check_key(key)?;
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::head_object(&client, bucket, key).await
}

/// 生成预签名 GET 链接。有效期上限 7 天（服务端限制）。
pub async fn presign_object(
    id: &str,
    bucket: &str,
    key: &str,
    expires_secs: Option<u32>,
) -> AppResult<PresignedUrl> {
    check_key(key)?;
    let secs = expires_secs.unwrap_or(DEFAULT_PRESIGN_SECS).clamp(60, MAX_PRESIGN_SECS);
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::presign_get(&client, bucket, key, secs).await
}

/// 读取对象开头的文本内容（文本预览）。
pub async fn read_object_text(
    id: &str,
    bucket: &str,
    key: &str,
    max_bytes: Option<u32>,
) -> AppResult<TextPreview> {
    check_key(key)?;
    let limit = max_bytes.unwrap_or(DEFAULT_TEXT_PREVIEW_BYTES).clamp(1, MAX_TEXT_PREVIEW_BYTES);
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::get_object_text(&client, bucket, key, limit as usize).await
}

/// 下载对象到本地路径。
pub async fn download_object(
    id: &str,
    bucket: &str,
    key: &str,
    dest_path: &str,
) -> AppResult<DownloadResult> {
    check_key(key)?;
    if dest_path.trim().is_empty() {
        return Err(AppError::Invalid("目标路径不能为空".to_string()));
    }
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    let size = s3::get_object_to_file(&client, bucket, key, Path::new(dest_path.trim())).await?;
    Ok(DownloadResult {
        path: dest_path.trim().to_string(),
        size,
    })
}

/// 删除对象。
pub async fn delete_object(id: &str, bucket: &str, key: &str) -> AppResult<()> {
    check_key(key)?;
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::delete_object(&client, bucket, key).await
}

/// 重命名对象：`new_name` 为本层新名称，目录部分保持不变。
///
/// S3 无原生 rename，实现为「服务端复制 + 删除源对象」。
pub async fn rename_object(
    id: &str,
    bucket: &str,
    key: &str,
    new_name: &str,
) -> AppResult<ObjectEntry> {
    check_key(key)?;
    let name = new_name.trim();
    if name.is_empty() {
        return Err(AppError::Invalid("新名称不能为空".to_string()));
    }
    if name.contains('/') {
        return Err(AppError::Invalid("新名称不能包含 /".to_string()));
    }

    let (parent, _) = split_key(key);
    let new_key = if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    };
    if new_key == key {
        return Err(AppError::Invalid("新名称与原名称相同".to_string()));
    }

    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::copy_object(&client, bucket, key, &new_key).await?;
    if let Err(e) = s3::delete_object(&client, bucket, key).await {
        // 复制成功但删除失败：源对象仍在，回报为错误，由用户决定后续处理，
        // 不做自动回滚（回滚可能误删已复制完成的新对象）。
        return Err(AppError::S3(format!("重命名完成但删除原对象失败：{e}")));
    }

    Ok(ObjectEntry {
        key: new_key.clone(),
        name: name.to_string(),
        is_dir: false,
        size: 0,
        last_modified: None,
        etag: None,
        storage_class: None,
    })
}

/// 重命名目录（公共前缀）。
///
/// S3 中目录不存在实体，重命名 = 前缀下所有对象依次「服务端复制 + 删除」。
/// 全程服务端 copy，不经过本地内存。
pub async fn rename_directory(
    id: &str,
    bucket: &str,
    prefix: &str,
    new_name: &str,
) -> AppResult<RenameDirResult> {
    if bucket.is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    if prefix.is_empty() {
        return Err(AppError::Invalid("不能重命名桶根目录".to_string()));
    }
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    let report = s3::rename_directory(&client, bucket, prefix, new_name).await?;
    Ok(RenameDirResult {
        moved: report.moved as u32,
        errors: report.errors,
    })
}

/// 读取对象标签。
pub async fn get_object_tags(
    id: &str,
    bucket: &str,
    key: &str,
) -> AppResult<Vec<ObjectTag>> {
    check_key(key)?;
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::get_object_tags(&client, bucket, key).await
}

/// 覆盖写入对象标签（整体替换）。
pub async fn put_object_tags(
    id: &str,
    bucket: &str,
    key: &str,
    tags: Vec<ObjectTag>,
) -> AppResult<Vec<ObjectTag>> {
    check_key(key)?;
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::put_object_tags(&client, bucket, key, &tags).await?;
    s3::get_object_tags(&client, bucket, key).await
}

/// 读取桶的访问级别与原始策略。
pub async fn get_bucket_access(id: &str, bucket: &str) -> AppResult<BucketPolicyInfo> {
    if bucket.is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::get_bucket_access(&client, bucket).await
}

/// 设置桶公开 / 私有。
pub async fn set_bucket_public(
    id: &str,
    bucket: &str,
    public: bool,
) -> AppResult<BucketPolicyInfo> {
    if bucket.is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;
    s3::set_bucket_public(&client, bucket, public).await
}

/// 上传本地文件与文件夹。
///
/// 文件夹会递归展开并按相对路径保持目录结构；全程在 Rust 侧流式读写，
/// 文件内容不经过前端内存与 IPC（开发文档 6.3）。
pub async fn upload_paths(
    id: &str,
    bucket: &str,
    prefix: &str,
    paths: Vec<String>,
) -> AppResult<UploadSummary> {
    if bucket.is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    if paths.is_empty() {
        return Err(AppError::Invalid("未选择任何文件或文件夹".to_string()));
    }

    let items = expand_upload_items(&paths, normalize_prefix(prefix))?;
    let profile = get_connection(id)?;
    let client = client_for(&profile).await?;

    let mut summary = UploadSummary::default();
    for (path, key) in items {
        match s3::put_object_file(&client, bucket, &key, &path).await {
            Ok(len) => {
                summary.uploaded += 1;
                summary.total_bytes += len;
            }
            Err(e) => {
                summary.failed += 1;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                summary.errors.push(format!("{name}：{e}"));
            }
        }
    }
    Ok(summary)
}

/// 文本预览默认读取上限（256 KiB）。
pub const DEFAULT_TEXT_PREVIEW_BYTES: u32 = 256 * 1024;
/// 文本预览读取上限硬顶（1 MiB）。
pub const MAX_TEXT_PREVIEW_BYTES: u32 = 1024 * 1024;
/// 预签名默认有效期（1 小时）。
pub const DEFAULT_PRESIGN_SECS: u32 = 3600;
/// 预签名有效期上限（7 天，服务端限制）。
pub const MAX_PRESIGN_SECS: u32 = 7 * 24 * 3600;

fn check_key(key: &str) -> AppResult<()> {
    if key.trim().is_empty() {
        return Err(AppError::Invalid("对象 key 不能为空".to_string()));
    }
    Ok(())
}

/// 拆分为「目录前缀」与「末段名称」。
fn split_key(key: &str) -> (&str, &str) {
    match key.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", key),
    }
}

/// 前缀规范化：非空时保证以 `/` 结尾。
fn normalize_prefix(prefix: &str) -> String {
    let p = prefix.trim().trim_start_matches('/');
    if p.is_empty() {
        String::new()
    } else if p.ends_with('/') {
        p.to_string()
    } else {
        format!("{p}/")
    }
}

/// 把本地路径展开为 `(本地路径, 对象 key)` 列表。
///
/// - 文件：key = 前缀 + 文件名
/// - 目录：递归展开，key = 前缀 + 目录名 + 相对路径
fn expand_upload_items(
    paths: &[String],
    prefix: String,
) -> AppResult<Vec<(std::path::PathBuf, String)>> {
    let mut items: Vec<(std::path::PathBuf, String)> = Vec::new();

    for raw in paths {
        let path = std::path::PathBuf::from(raw.trim());
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| AppError::Invalid(format!("无法访问 {raw}：{e}")))?;

        if meta.is_file() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            items.push((path.clone(), format!("{prefix}{name}")));
        } else if meta.is_dir() {
            let dir_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut files = Vec::new();
            walk_dir(&path, &mut files)
                .map_err(|e| AppError::Invalid(format!("遍历目录失败：{e}")))?;
            for file in files {
                let rel = file
                    .strip_prefix(&path)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| {
                        file.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    });
                items.push((file, format!("{prefix}{dir_name}/{rel}")));
            }
        }
    }

    if items.is_empty() {
        return Err(AppError::Invalid(
            "所选路径中没有可上传的文件".to_string(),
        ));
    }
    Ok(items)
}

/// 递归收集目录下的文件（跳过符号链接，避免循环）。
fn walk_dir(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            walk_dir(&path, out)?;
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_key_returns_parent_and_name() {
        assert_eq!(split_key("a/b/c.txt"), ("a/b", "c.txt"));
        assert_eq!(split_key("root.txt"), ("", "root.txt"));
    }

    #[test]
    fn normalize_prefix_always_ends_with_slash() {
        assert_eq!(normalize_prefix(""), "");
        assert_eq!(normalize_prefix("photos"), "photos/");
        assert_eq!(normalize_prefix("photos/"), "photos/");
        assert_eq!(normalize_prefix("/photos"), "photos/");
    }

    #[test]
    fn rename_keeps_directory_and_replaces_name() {
        // 复刻 rename_object 中的 key 计算，避免为测试引入网络依赖
        let compute = |key: &str, new_name: &str| {
            let (parent, _) = split_key(key);
            if parent.is_empty() {
                new_name.to_string()
            } else {
                format!("{parent}/{new_name}")
            }
        };
        assert_eq!(compute("a/b/old.png", "new.png"), "a/b/new.png");
        assert_eq!(compute("old.png", "new.png"), "new.png");
    }

    #[test]
    fn bucket_name_rules_are_enforced() {
        assert!(validate_bucket_name("my-bucket").is_ok());
        assert!(validate_bucket_name("a.b.c-1").is_ok());

        assert!(validate_bucket_name("").is_err());
        assert!(validate_bucket_name("ab").is_err());
        assert!(validate_bucket_name("MyBucket").is_err());
        assert!(validate_bucket_name("-bucket").is_err());
        assert!(validate_bucket_name("bucket-").is_err());
        assert!(validate_bucket_name("my_bucket").is_err());
        assert!(validate_bucket_name("my..bucket").is_err());
        assert!(validate_bucket_name("192.168.1.1").is_err());
        assert!(validate_bucket_name(&"a".repeat(64)).is_err());
    }

    #[test]
    fn upload_of_empty_selection_is_rejected() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(upload_paths("nope", "bucket", "", vec![]));
        assert!(matches!(err, Err(AppError::Invalid(_))));
    }
}
