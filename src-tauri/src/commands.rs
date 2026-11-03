//! Tauri command 薄封装：转发到 `s3warden-core`，不包含业务逻辑。
//!
//! 约定：同步核心函数用同步 command；涉及网络/IO 的用异步 command（由 Tauri 内置 tokio 运行时执行）。

use s3warden_core::api;
use s3warden_core::error::AppError;
use s3warden_core::types::{
    BackendPresetInfo, BatchDeleteResult, BucketDeleteResult, BucketPolicyInfo, BucketSummary,
    ConnectionInput, ConnectionPatch,
    ConnectionProfile, ConnectionTestOk, DownloadResult, ObjectDetail, ObjectEntry, ObjectPage,
    ObjectTag, PresignedUrl, RenameDirResult, SecretStatus, TextPreview, UploadSummary,
};

/// 后端接入类型选项（新建连接表单）。
#[tauri::command]
#[specta::specta]
pub fn list_backend_presets() -> Vec<BackendPresetInfo> {
    api::list_backend_presets()
}

#[tauri::command]
#[specta::specta]
pub async fn create_connection(input: ConnectionInput) -> Result<ConnectionProfile, AppError> {
    api::create_connection(input).await
}

#[tauri::command]
#[specta::specta]
pub fn list_connections() -> Result<Vec<ConnectionProfile>, AppError> {
    api::list_connections()
}

#[tauri::command]
#[specta::specta]
pub fn update_connection(
    id: String,
    patch: ConnectionPatch,
) -> Result<ConnectionProfile, AppError> {
    api::update_connection(&id, patch)
}

#[tauri::command]
#[specta::specta]
pub fn delete_connection(id: String) -> Result<(), AppError> {
    api::delete_connection(&id)
}

#[tauri::command]
#[specta::specta]
pub fn set_default_connection(id: String) -> Result<(), AppError> {
    api::set_default_connection(&id)
}

#[tauri::command]
#[specta::specta]
pub fn secret_status(id: String) -> Result<SecretStatus, AppError> {
    api::secret_status(&id)
}

/// 读取 Secret Key 明文（仅用于「编辑连接」回填，其余链路不回传明文）。
#[tauri::command]
#[specta::specta]
pub fn reveal_secret(id: String) -> Result<String, AppError> {
    api::reveal_secret(&id)
}

#[tauri::command]
#[specta::specta]
pub async fn test_new_connection(
    input: ConnectionInput,
) -> Result<ConnectionTestOk, AppError> {
    api::test_new_connection(input).await
}

#[tauri::command]
#[specta::specta]
pub async fn test_saved_connection(id: String) -> Result<ConnectionTestOk, AppError> {
    api::test_saved_connection(&id).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_buckets(id: String) -> Result<Vec<BucketSummary>, AppError> {
    api::list_buckets(&id).await
}

/// 在当前连接下创建桶（区域沿用连接档案配置）。
#[tauri::command]
#[specta::specta]
pub async fn create_bucket(id: String, bucket: String) -> Result<(), AppError> {
    api::create_bucket(&id, &bucket).await
}

/// 删除桶。`force = true` 时先清空桶内全部对象再删桶。
#[tauri::command]
#[specta::specta]
pub async fn delete_bucket(
    id: String,
    bucket: String,
    force: bool,
) -> Result<BucketDeleteResult, AppError> {
    api::delete_bucket(&id, &bucket, force).await
}

#[tauri::command]
#[specta::specta]
pub async fn rename_directory(
    id: String,
    bucket: String,
    prefix: String,
    new_name: String,
) -> Result<RenameDirResult, AppError> {
    api::rename_directory(&id, &bucket, &prefix, &new_name).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_object_tags(
    id: String,
    bucket: String,
    key: String,
) -> Result<Vec<ObjectTag>, AppError> {
    api::get_object_tags(&id, &bucket, &key).await
}

#[tauri::command]
#[specta::specta]
pub async fn put_object_tags(
    id: String,
    bucket: String,
    key: String,
    tags: Vec<ObjectTag>,
) -> Result<Vec<ObjectTag>, AppError> {
    api::put_object_tags(&id, &bucket, &key, tags).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_bucket_access(
    id: String,
    bucket: String,
) -> Result<BucketPolicyInfo, AppError> {
    api::get_bucket_access(&id, &bucket).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_bucket_public(
    id: String,
    bucket: String,
    is_public: bool,
) -> Result<BucketPolicyInfo, AppError> {
    api::set_bucket_public(&id, &bucket, is_public).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_objects(
    id: String,
    bucket: String,
    prefix: String,
    token: Option<String>,
) -> Result<ObjectPage, AppError> {
    api::list_objects(&id, &bucket, &prefix, token).await
}

/// 对象元数据（详情面板）。
#[tauri::command]
#[specta::specta]
pub async fn stat_object(
    id: String,
    bucket: String,
    key: String,
) -> Result<ObjectDetail, AppError> {
    api::stat_object(&id, &bucket, &key).await
}

/// 预签名 GET 链接（下载 / 分享 / 图片预览共用）。
#[tauri::command]
#[specta::specta]
pub async fn presign_object(
    id: String,
    bucket: String,
    key: String,
    expires_secs: Option<u32>,
) -> Result<PresignedUrl, AppError> {
    api::presign_object(&id, &bucket, &key, expires_secs).await
}

/// 读取对象开头的文本内容（文本预览）。
#[tauri::command]
#[specta::specta]
pub async fn read_object_text(
    id: String,
    bucket: String,
    key: String,
    max_bytes: Option<u32>,
) -> Result<TextPreview, AppError> {
    api::read_object_text(&id, &bucket, &key, max_bytes).await
}

/// 下载对象到指定本地路径。
#[tauri::command]
#[specta::specta]
pub async fn download_object(
    id: String,
    bucket: String,
    key: String,
    dest_path: String,
) -> Result<DownloadResult, AppError> {
    api::download_object(&id, &bucket, &key, &dest_path).await
}

/// 删除对象。
#[tauri::command]
#[specta::specta]
pub async fn delete_object(id: String, bucket: String, key: String) -> Result<(), AppError> {
    api::delete_object(&id, &bucket, &key).await
}

/// 重命名对象（服务端复制 + 删除源对象）；`new_name` 为本层新名称。
#[tauri::command]
#[specta::specta]
pub async fn rename_object(
    id: String,
    bucket: String,
    key: String,
    new_name: String,
) -> Result<ObjectEntry, AppError> {
    api::rename_object(&id, &bucket, &key, &new_name).await
}

/// 上传本地文件与文件夹（文件夹递归展开，保持相对目录结构）。
#[tauri::command]
#[specta::specta]
pub async fn upload_paths(
    id: String,
    bucket: String,
    prefix: String,
    paths: Vec<String>,
) -> Result<UploadSummary, AppError> {
    api::upload_paths(&id, &bucket, &prefix, paths).await
}

/// 创建「文件夹」占位前缀（OBJ-13）。
#[tauri::command]
#[specta::specta]
pub async fn create_prefix(
    id: String,
    bucket: String,
    prefix: String,
    name: String,
) -> Result<ObjectEntry, AppError> {
    api::create_prefix(&id, &bucket, &prefix, &name).await
}

/// 批量删除对象（目录自动展开为其下全部对象；OBJ-10）。
#[tauri::command]
#[specta::specta]
pub async fn delete_objects_batch(
    id: String,
    bucket: String,
    keys: Vec<String>,
) -> Result<BatchDeleteResult, AppError> {
    api::delete_objects_batch(&id, &bucket, keys).await
}
