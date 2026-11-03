//! S3 操作层：客户端构造、桶列表、前缀浏览。
//!
//! 仅使用高层 S3 API；跨后端差异（MinIO / RustFS / Generic）由统一的客户端构造参数吸收。

use std::path::Path;
use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration, Delete, ObjectIdentifier};
pub use aws_sdk_s3::Client;
use aws_smithy_types::byte_stream::ByteStream;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::types::{
    BucketAccess, BucketPolicyInfo, BucketSummary, ObjectDetail, ObjectEntry, ObjectPage,
    ObjectTag, PresignedUrl, TextPreview,
};

/// 未指定区域时的默认值（MinIO / RustFS 均习惯使用 us-east-1）。
pub const DEFAULT_REGION: &str = "us-east-1";

/// 单次 list_objects 页大小。
const LIST_PAGE_SIZE: i32 = 200;

/// 单次 DeleteObjects 最多 key 数（服务端限制 1000）。
const DELETE_BATCH_SIZE: usize = 1000;

/// S3 请求超时（秒）。
pub const REQUEST_TIMEOUT_SECS: u64 = 20;

/// 构造 S3 客户端（显式凭据 + 端点覆盖，不读取环境默认链）。
pub fn build_client(
    endpoint: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
    path_style: bool,
) -> Client {
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            access_key,
            secret_key,
            None,
            None,
            "s3warden",
        ))
        .endpoint_url(endpoint)
        .region(Region::new(region.to_string()))
        .force_path_style(path_style)
        .build();
    Client::from_conf(config)
}

fn rfc3339(dt: Option<&aws_smithy_types::DateTime>) -> Option<String> {
    dt.and_then(|d| {
        time::OffsetDateTime::from_unix_timestamp(d.secs())
            .ok()
            .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok())
    })
}

/// 列出全部桶。
pub async fn list_buckets(client: &Client) -> AppResult<Vec<BucketSummary>> {
    let out = tokio::time::timeout(
        Duration::from_secs(REQUEST_TIMEOUT_SECS),
        client.list_buckets().send(),
    )
    .await
    .map_err(|_| AppError::Network("获取桶列表超时".to_string()))?
    .map_err(|e| AppError::S3(format!("获取桶列表失败：{e}")))?;

    Ok(out
        .buckets()
        .iter()
        .filter_map(|b| {
            b.name().map(|n| BucketSummary {
                name: n.to_string(),
                created_at: rfc3339(b.creation_date()),
            })
        })
        .collect())
}

/// 创建桶。
///
/// 区域为 `us-east-1` 时不带 CreateBucketConfiguration（AWS 语义即默认区域），
/// 其余区域显式声明 LocationConstraint。MinIO / RustFS 通常使用 us-east-1，
/// 因此默认路径不会因 LocationConstraint 不被识别而失败。
pub async fn create_bucket(client: &Client, bucket: &str, region: &str) -> AppResult<()> {
    let mut req = client.create_bucket().bucket(bucket);
    if !region.is_empty() && region != DEFAULT_REGION {
        req = req.create_bucket_configuration(
            CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::from(region))
                .build(),
        );
    }

    tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), req.send())
        .await
        .map_err(|_| AppError::Network("创建桶超时".to_string()))?
        .map_err(|e| AppError::S3(format!("创建桶失败：{e}")))?;
    Ok(())
}

/// 删除空桶。非空桶服务端会直接拒绝（BucketNotEmpty）。
pub async fn delete_bucket(client: &Client, bucket: &str) -> AppResult<()> {
    tokio::time::timeout(
        Duration::from_secs(REQUEST_TIMEOUT_SECS),
        client.delete_bucket().bucket(bucket).send(),
    )
    .await
    .map_err(|_| AppError::Network("删除桶超时".to_string()))?
    .map_err(|e| AppError::S3(format!("删除桶失败：{e}")))?;
    Ok(())
}

/// 批量删除对象（DeleteObjects，每批最多 1000 个 key）。
///
/// 用于「强制删除非空桶」：先清空再删桶，全程服务端操作。
pub async fn delete_objects(client: &Client, bucket: &str, keys: &[String]) -> AppResult<usize> {
    let mut removed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for chunk in keys.chunks(DELETE_BATCH_SIZE) {
        let ids: Vec<ObjectIdentifier> = chunk
            .iter()
            .filter_map(|k| ObjectIdentifier::builder().key(k).build().ok())
            .collect();
        if ids.is_empty() {
            continue;
        }
        let del = Delete::builder()
            .set_objects(Some(ids))
            .build()
            .map_err(|e| AppError::S3(format!("构造批量删除请求失败：{e}")))?;

        let out = tokio::time::timeout(
            Duration::from_secs(REQUEST_TIMEOUT_SECS),
            client.delete_objects().bucket(bucket).delete(del).send(),
        )
        .await
        .map_err(|_| AppError::Network("清空对象超时".to_string()))?
        .map_err(|e| AppError::S3(format!("清空对象失败：{e}")))?;

        removed += out.deleted().len();
        for e in out.errors() {
            failures.push(format!("{}：{}", e.key().unwrap_or("?"), e.message().unwrap_or_default()));
        }
    }

    if !failures.is_empty() {
        return Err(AppError::S3(format!(
            "清空对象失败（{} 项）：{}",
            failures.len(),
            failures.join("；")
        )));
    }
    Ok(removed)
}

/// 按前缀浏览对象（delimiter="/"，服务端分页，OBJ-01/04）。
pub async fn list_objects(
    client: &Client,
    bucket: &str,
    prefix: &str,
    token: Option<&str>,
) -> AppResult<ObjectPage> {
    let mut req = client
        .list_objects_v2()
        .bucket(bucket)
        .delimiter("/")
        .max_keys(LIST_PAGE_SIZE);
    if !prefix.is_empty() {
        req = req.prefix(prefix);
    }
    if let Some(t) = token {
        req = req.continuation_token(t);
    }

    let out = tokio::time::timeout(
        Duration::from_secs(REQUEST_TIMEOUT_SECS),
        req.send(),
    )
    .await
    .map_err(|_| AppError::Network("获取对象列表超时".to_string()))?
    .map_err(|e| AppError::S3(format!("获取对象列表失败：{e}")))?;

    let mut entries: Vec<ObjectEntry> = Vec::new();
    for cp in out.common_prefixes() {
        let Some(p) = cp.prefix() else { continue };
        if p != prefix {
            entries.push(dir_entry(p));
        }
    }
    for obj in out.contents() {
        let Some(key) = obj.key() else { continue };
        // 跳过代表当前目录自身的占位对象
        if !prefix.is_empty() && key == prefix {
            continue;
        }
        entries.push(object_entry(
            key,
            prefix,
            obj.size().unwrap_or(0).max(0) as u64,
            rfc3339(obj.last_modified()),
            obj.e_tag().map(str::to_string),
            obj.storage_class().map(|s| s.as_str().to_string()),
        ));
    }

    // 目录在前，同级按名称不区分大小写排序
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(ObjectPage {
        entries,
        next_token: out.next_continuation_token().map(str::to_string),
    })
}

/// 由公共前缀构造目录条目。
pub fn dir_entry(prefix: &str) -> ObjectEntry {
    ObjectEntry {
        key: prefix.to_string(),
        name: dir_name(prefix),
        is_dir: true,
        size: 0,
        last_modified: None,
        etag: None,
        storage_class: None,
    }
}

/// 目录显示名：去掉结尾斜杠后取最后一段。
pub fn dir_name(prefix: &str) -> String {
    prefix
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(prefix)
        .to_string()
}

/// 构造对象条目。
#[allow(clippy::too_many_arguments)]
pub fn object_entry(
    key: &str,
    prefix: &str,
    size: u64,
    last_modified: Option<String>,
    etag: Option<String>,
    storage_class: Option<String>,
) -> ObjectEntry {
    ObjectEntry {
        key: key.to_string(),
        name: object_name(key, prefix),
        is_dir: false,
        size,
        last_modified,
        etag,
        storage_class,
    }
}

/// 对象显示名：去掉前缀后取最后一段。
pub fn object_name(key: &str, prefix: &str) -> String {
    let stripped = key.strip_prefix(prefix).unwrap_or(key);
    stripped.rsplit('/').next().unwrap_or(stripped).to_string()
}

/// 扩展名 → Content-Type（够用即止，避免为上传引入额外依赖）。
pub fn guess_content_type(name: &str) -> &'static str {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain; charset=utf-8",
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "ts" => "text/plain; charset=utf-8",
        "yml" | "yaml" => "application/yaml",
        "toml" => "application/toml",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "tar" => "application/x-tar",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// 读取对象元数据（详情面板数据源）。
pub async fn head_object(client: &Client, bucket: &str, key: &str) -> AppResult<ObjectDetail> {
    let out = tokio::time::timeout(
        Duration::from_secs(REQUEST_TIMEOUT_SECS),
        client.head_object().bucket(bucket).key(key).send(),
    )
    .await
    .map_err(|_| AppError::Network("获取对象元数据超时".to_string()))?
    .map_err(|e| AppError::S3(format!("获取对象元数据失败：{e}")))?;

    Ok(ObjectDetail {
        key: key.to_string(),
        name: object_name(key, ""),
        size: out.content_length().unwrap_or(0).max(0) as u64,
        last_modified: rfc3339(out.last_modified()),
        etag: out.e_tag().map(str::to_string),
        content_type: out.content_type().map(str::to_string),
        storage_class: out.storage_class().map(|s| s.as_str().to_string()),
    })
}

/// 上传本地文件（流式读取，不整份载入内存，开发文档 6.3）。
pub async fn put_object_file(
    client: &Client,
    bucket: &str,
    key: &str,
    path: &Path,
) -> AppResult<u64> {
    let len = std::fs::metadata(path)
        .map_err(|e| AppError::Invalid(format!("读取本地文件失败：{e}")))?
        .len();
    let body = ByteStream::from_path(path)
        .await
        .map_err(|e| AppError::Invalid(format!("打开本地文件失败：{e}")))?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .content_type(guess_content_type(&file_name))
        .content_length(len as i64)
        .body(body)
        .send()
        .await
        .map_err(|e| AppError::S3(format!("上传 {key} 失败：{e}")))?;
    Ok(len)
}

/// 下载对象到本地路径（流式写盘，不整份载入内存）。
pub async fn get_object_to_file(
    client: &Client,
    bucket: &str,
    key: &str,
    dest: &Path,
) -> AppResult<u64> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Invalid(format!("创建目标目录失败：{e}")))?;
        }
    }

    let resp = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| AppError::S3(format!("下载 {key} 失败：{e}")))?;

    let mut reader = resp.body.into_async_read();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| AppError::Invalid(format!("创建文件失败：{e}")))?;
    let written = tokio::io::copy(&mut reader, &mut file)
        .await
        .map_err(|e| AppError::Invalid(format!("写入文件失败：{e}")))?;
    file.flush()
        .await
        .map_err(|e| AppError::Invalid(format!("写入文件失败：{e}")))?;
    Ok(written)
}

/// 服务端复制（重命名实现的底层操作）。
///
/// `copy_source` 要求 URL 编码的 `/{bucket}/{key}`，故对 key 做 percent 编码。
pub async fn copy_object(
    client: &Client,
    bucket: &str,
    src_key: &str,
    dst_key: &str,
) -> AppResult<()> {
    let source = format!("/{}/{}", percent_encode(bucket), percent_encode(src_key));
    client
        .copy_object()
        .bucket(bucket)
        .copy_source(source)
        .key(dst_key)
        .send()
        .await
        .map_err(|e| AppError::S3(format!("复制对象失败：{e}")))?;
    Ok(())
}

/// 删除单个对象。
pub async fn delete_object(client: &Client, bucket: &str, key: &str) -> AppResult<()> {
    client
        .delete_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| AppError::S3(format!("删除 {key} 失败：{e}")))?;
    Ok(())
}

/// 生成预签名 GET 链接（下载 / 分享 / 图片预览共用，不消耗前端内存）。
pub async fn presign_get(
    client: &Client,
    bucket: &str,
    key: &str,
    expires_secs: u32,
) -> AppResult<PresignedUrl> {
    let cfg = PresigningConfig::expires_in(Duration::from_secs(expires_secs as u64))
        .map_err(|e| AppError::Invalid(format!("预签名有效期不合法：{e}")))?;
    let req = tokio::time::timeout(
        Duration::from_secs(REQUEST_TIMEOUT_SECS),
        client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(cfg),
    )
    .await
    .map_err(|_| AppError::Network("生成预签名链接超时".to_string()))?
    .map_err(|e| AppError::S3(format!("生成预签名链接失败：{e}")))?;

    Ok(PresignedUrl {
        url: req.uri().to_string(),
        expires_in_secs: expires_secs,
    })
}

/// 读取对象开头的文本内容（按 Range 请求，只取前 `max_bytes` 字节）。
pub async fn get_object_text(
    client: &Client,
    bucket: &str,
    key: &str,
    max_bytes: usize,
) -> AppResult<TextPreview> {
    let limit = max_bytes.max(1);
    let resp = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .range(format!("bytes=0-{}", limit - 1))
        .send()
        .await
        .map_err(|e| AppError::S3(format!("读取 {key} 失败：{e}")))?;

    // 先取 content_range 再消费 body（collect 会移动 body）。
    let total = resp
        .content_range()
        .and_then(|r| r.rsplit('/').next())
        .and_then(|n| n.parse::<usize>().ok());

    let data = resp
        .body
        .collect()
        .await
        .map_err(|e| AppError::S3(format!("读取 {key} 内容失败：{e}")))?
        .into_bytes();

    let truncated = match total {
        Some(t) => t > data.len(),
        None => data.len() >= limit,
    };
    let binary = data.iter().take(4096).any(|b| *b == 0);

    Ok(TextPreview {
        content: String::from_utf8_lossy(&data).to_string(),
        truncated,
        binary,
    })
}

// ---------------------------------------------------------------------------
// 对象标签（Object Tagging）
// ---------------------------------------------------------------------------

/// 读取对象标签。
///
/// S3 无标签时部分实现会返回 `NoSuchTagSet`，按空集合处理。
pub async fn get_object_tags(
    client: &Client,
    bucket: &str,
    key: &str,
) -> AppResult<Vec<ObjectTag>> {
    let res = client
        .get_object_tagging()
        .bucket(bucket)
        .key(key)
        .send()
        .await;
    let out = match res {
        Ok(out) => out,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("NoSuchTagSet") || msg.contains("NoSuchTagSetError") {
                return Ok(Vec::new());
            }
            return Err(AppError::S3(format!("读取 {key} 标签失败：{msg}")));
        }
    };
    Ok(out
        .tag_set()
        .iter()
        .map(|t| ObjectTag {
            key: t.key().to_string(),
            value: t.value().to_string(),
        })
        .collect())
}

/// 覆盖写入对象标签（S3 语义：整体替换，上限 10 组）。
pub async fn put_object_tags(
    client: &Client,
    bucket: &str,
    key: &str,
    tags: &[ObjectTag],
) -> AppResult<()> {
    if tags.len() > 10 {
        return Err(AppError::Invalid("标签最多 10 组".to_string()));
    }
    for t in tags {
        if t.key.trim().is_empty() {
            return Err(AppError::Invalid("标签名不能为空".to_string()));
        }
        if t.key.len() > 128 || t.value.len() > 256 {
            return Err(AppError::Invalid(
                "标签名最长 128 字符，值最长 256 字符".to_string(),
            ));
        }
    }

    let mut tagging = aws_sdk_s3::types::Tagging::builder();
    for t in tags {
        tagging = tagging.tag_set(
            aws_sdk_s3::types::Tag::builder()
                .key(t.key.trim())
                .value(t.value.trim())
                .build()
                .map_err(|e| AppError::S3(format!("构造标签失败：{e}")))?,
        );
    }
    let tagging = tagging
        .build()
        .map_err(|e| AppError::S3(format!("构造标签集合失败：{e}")))?;

    client
        .put_object_tagging()
        .bucket(bucket)
        .key(key)
        .tagging(tagging)
        .send()
        .await
        .map_err(|e| AppError::S3(format!("写入 {key} 标签失败：{e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 桶策略（公开 / 私有）
// ---------------------------------------------------------------------------

/// 全员只读策略模板（AWS IAM Policy 语法，MinIO / RustFS / 云厂商通用）。
pub fn public_read_policy(bucket: &str) -> String {
    format!(
        concat!(
            r#"{{"Version":"2012-10-17","Statement":["#,
            r#"{{"Effect":"Allow","Principal":{{"AWS":["*"]}},"#,
            r#""Action":["s3:GetObject"],"#,
            r#""Resource":["arn:aws:s3:::{bucket}/*"]}}]"#,
            r#"}}"#
        ),
        bucket = bucket
    )
}

/// 归一化策略 JSON（去空白）以便与模板比较。
fn normalize_policy(json: &str) -> String {
    json.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 读取桶策略并判定访问级别。
///
/// 无策略 → 私有；等于全员只读模板 → 公开；其他 → 自定义（前端提示不要覆盖）。
pub async fn get_bucket_access(
    client: &Client,
    bucket: &str,
) -> AppResult<BucketPolicyInfo> {
    let res = client.get_bucket_policy().bucket(bucket).send().await;
    let json = match res {
        Ok(out) => out.policy().map(str::to_string),
        Err(e) => {
            // `SdkError` 的 Display 只输出 "service error"，必须取服务错误码 / HTTP 状态判断。
            let (code, message) = match e.as_service_error() {
                Some(se) => (
                    se.code().unwrap_or_default().to_string(),
                    se.message().unwrap_or_default().to_string(),
                ),
                None => (String::new(), e.to_string()),
            };
            let not_found = e
                .raw_response()
                .map(|r| r.status().as_u16() == 404)
                .unwrap_or(false);

            if code == "NoSuchBucketPolicy"
                || code == "NoSuchBucketPolicyError"
                || code == "NotFound"
                || not_found
            {
                // 无策略 = 私有（S3 默认语义）
                None
            } else {
                return Err(AppError::S3(format!(
                    "读取桶策略失败：{}{}",
                    if code.is_empty() {
                        String::new()
                    } else {
                        format!("{code} ")
                    },
                    if message.is_empty() { e.to_string() } else { message }
                )));
            }
        }
    };

    let access = match &json {
        None => BucketAccess::Private,
        Some(j) => {
            if normalize_policy(j) == normalize_policy(&public_read_policy(bucket)) {
                BucketAccess::Public
            } else {
                BucketAccess::Custom
            }
        }
    };
    Ok(BucketPolicyInfo {
        access,
        policy_json: json,
    })
}

/// 设置桶访问级别：公开 = 写入全员只读策略；私有 = 删除桶策略。
pub async fn set_bucket_public(
    client: &Client,
    bucket: &str,
    public: bool,
) -> AppResult<BucketPolicyInfo> {
    if public {
        let policy = public_read_policy(bucket);
        client
            .put_bucket_policy()
            .bucket(bucket)
            .policy(policy)
            .send()
            .await
            .map_err(|e| AppError::S3(format!("设置公开策略失败：{e}")))?;
    } else {
        client
            .delete_bucket_policy()
            .bucket(bucket)
            .send()
            .await
            .map_err(|e| AppError::S3(format!("设置私有（删除策略）失败：{e}")))?;
    }
    get_bucket_access(client, bucket).await
}

// ---------------------------------------------------------------------------
// 目录（前缀）重命名
// ---------------------------------------------------------------------------

/// 列出某前缀下的**全部**对象 key（不使用 delimiter，含所有层级）。
pub async fn list_all_keys(
    client: &Client,
    bucket: &str,
    prefix: &str,
) -> AppResult<Vec<String>> {
    let mut keys = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_objects_v2().bucket(bucket).prefix(prefix);
        if let Some(t) = &token {
            req = req.continuation_token(t);
        }
        let out = req
            .send()
            .await
            .map_err(|e| AppError::S3(format!("列举 {prefix} 下对象失败：{e}")))?;
        for obj in out.contents() {
            if let Some(k) = obj.key() {
                keys.push(k.to_string());
            }
        }
        match out.next_continuation_token() {
            Some(t) if !t.is_empty() => token = Some(t.to_string()),
            _ => break,
        }
    }
    Ok(keys)
}

/// 前缀重命名结果。
pub struct RenameDirReport {
    /// 成功移动的对象数。
    pub moved: usize,
    /// 失败项说明（最多保留 5 条）。
    pub errors: Vec<String>,
}

/// 重命名目录：把前缀下所有对象 copy 到新前缀再删除旧对象。
///
/// S3 没有「目录」实体，目录只是公共前缀，因此重命名必须逐个对象搬运；
/// 全程服务端 copy，不经过本地内存。
pub async fn rename_directory(
    client: &Client,
    bucket: &str,
    prefix: &str,
    new_name: &str,
) -> AppResult<RenameDirReport> {
    let normalized = if prefix.is_empty() || prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    let (parent, old_name) = split_prefix(&normalized);
    let new_prefix = format!("{parent}{new_name}/");

    if new_name.trim().is_empty() || new_name.contains('/') {
        return Err(AppError::Invalid("目录名不能为空且不能包含 /".to_string()));
    }
    if new_prefix == normalized {
        return Err(AppError::Invalid("新名称与原名称相同".to_string()));
    }

    let keys = list_all_keys(client, bucket, &normalized).await?;
    let mut moved = 0usize;
    let mut errors = Vec::new();
    for key in &keys {
        let dst = format!("{new_prefix}{}", &key[normalized.len()..]);
        match copy_object(client, bucket, key, &dst).await {
            Ok(()) => {
                if let Err(e) = delete_object(client, bucket, key).await {
                    errors.push(format!("{key}：已复制但删除失败（{e}）"));
                } else {
                    moved += 1;
                }
            }
            Err(e) => errors.push(format!("{key}：{e}")),
        }
        if errors.len() >= 5 {
            break;
        }
    }
    let _ = old_name;
    Ok(RenameDirReport { moved, errors })
}

/// 拆分前缀为「父前缀 + 当前目录名」：`a/b/c/` → `("a/b/", "c")`。
pub fn split_prefix(prefix: &str) -> (String, String) {
    let trimmed = prefix.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(idx) => (trimmed[..idx + 1].to_string(), trimmed[idx + 1..].to_string()),
        None => (String::new(), trimmed.to_string()),
    }
}

/// 最小 percent 编码：保留 unreserved 字符与 `/`，其余按字节转义。
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_prefix_returns_parent_and_name() {
        assert_eq!(split_prefix("a/b/c/"), ("a/b/".to_string(), "c".to_string()));
        assert_eq!(split_prefix("top/"), (String::new(), "top".to_string()));
        assert_eq!(split_prefix(""), (String::new(), String::new()));
    }

    #[test]
    fn public_read_policy_matches_template_after_normalize() {
        let p = public_read_policy("demo");
        assert!(p.contains("arn:aws:s3:::demo/*"));
        assert!(p.contains("s3:GetObject"));
        assert_eq!(normalize_policy(&p), normalize_policy(&public_read_policy("demo")));
        assert_ne!(normalize_policy(&p), normalize_policy(&public_read_policy("other")));
    }

    #[test]
    fn dir_name_takes_last_segment() {
        assert_eq!(dir_name("a/b/c/"), "c");
        assert_eq!(dir_name("top/"), "top");
        assert_eq!(dir_name("top"), "top");
    }

    #[test]
    fn object_name_strips_prefix() {
        assert_eq!(object_name("a/b/x.txt", "a/b/"), "x.txt");
        assert_eq!(object_name("root.txt", ""), "root.txt");
        assert_eq!(object_name("a/b/c.txt", "a/b"), "c.txt");
    }

    #[test]
    fn dir_entry_is_folder_like() {
        let e = dir_entry("photos/2026/");
        assert!(e.is_dir);
        assert_eq!(e.name, "2026");
        assert_eq!(e.key, "photos/2026/");
    }

    #[test]
    fn content_type_maps_common_extensions() {
        assert_eq!(guess_content_type("a.PNG"), "image/png");
        assert_eq!(guess_content_type("a.JPG"), "image/jpeg");
        assert_eq!(guess_content_type("notes.txt"), "text/plain; charset=utf-8");
        assert_eq!(guess_content_type("data"), "application/octet-stream");
    }

    #[test]
    fn percent_encode_keeps_unreserved_and_slash() {
        assert_eq!(percent_encode("bucket/a-b_c.txt"), "bucket/a-b_c.txt");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("中文.png"), "%E4%B8%AD%E6%96%87.png");
    }
}
