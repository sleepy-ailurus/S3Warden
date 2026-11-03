//! 跨 IPC 的数据模型。
//!
//! 约定：
//! - 全部类型 `#[serde(rename_all = "camelCase")]`，与前端 TS 命名一致；
//! - **任何类型都不携带 Secret Key**（开发文档 2.6）；
//! - 时间一律 RFC3339 字符串；对象大小用 `u64`（导出绑定时映射为 number，S3 对象上限 5TB 远小于 2^53）。

use serde::{Deserialize, Serialize};
use specta::Type;

/// S3 兼容后端类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// MinIO。
    Minio,
    /// RustFS。
    Rustfs,
    /// 其他 S3 兼容端点。
    GenericS3,
}

impl BackendKind {
    /// 依据 HTTP `Server` 响应头识别后端（大小写不敏感）。
    pub fn from_server_header(value: &str) -> Self {
        let v = value.to_ascii_lowercase();
        if v.contains("minio") {
            Self::Minio
        } else if v.contains("rustfs") {
            Self::Rustfs
        } else {
            Self::GenericS3
        }
    }
}

/// 创建连接时用户选择的后端类型。
///
/// 与 [`BackendKind`] 的区别：`BackendPreset` 是**用户显式选择的接入类型**（决定默认端点/区域/寻址方式），
/// `BackendKind` 是**探测识别出的实际后端**（决定能力开关）。新增厂商只需在此增加一个变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BackendPreset {
    /// MinIO。
    Minio,
    /// RustFS。
    Rustfs,
    /// 阿里云 OSS。
    AliyunOss,
    /// 腾讯云 COS。
    TencentCos,
    /// 其他 S3 兼容端点（默认）。
    #[default]
    GenericS3,
}

impl BackendPreset {
    /// 该后端的默认寻址方式：云厂商用 virtual-host，自建用 path-style。
    pub fn default_path_style(self) -> bool {
        !matches!(self, Self::AliyunOss | Self::TencentCos)
    }

    /// 端点输入框占位示例。
    pub fn endpoint_hint(self) -> &'static str {
        match self {
            Self::Minio => "http://127.0.0.1:9000",
            Self::Rustfs => "http://127.0.0.1:9000",
            Self::AliyunOss => "https://oss-cn-hangzhou.aliyuncs.com",
            Self::TencentCos => "https://cos.ap-guangzhou.myqcloud.com",
            Self::GenericS3 => "https://s3.example.com",
        }
    }

    /// 区域输入框占位示例。
    pub fn region_hint(self) -> &'static str {
        match self {
            Self::AliyunOss => "oss-cn-hangzhou",
            Self::TencentCos => "ap-guangzhou",
            _ => "us-east-1（留空使用默认）",
        }
    }

    /// 预设 → 预期的探测后端（通用 S3 无预期值）。
    pub fn expected_backend(self) -> Option<BackendKind> {
        match self {
            Self::Minio => Some(BackendKind::Minio),
            Self::Rustfs => Some(BackendKind::Rustfs),
            _ => None,
        }
    }

    /// 该后端的接入须知（展示在新建连接对话框）。
    pub fn notes(self) -> &'static str {
        match self {
            Self::Minio => "支持完整管理控制台（用户 / Access Key / 策略 / 分组）",
            Self::Rustfs => "对象操作可用；管理控制台能力待适配",
            Self::AliyunOss => "云厂商端点，使用 virtual-host 寻址；管理控制台不可用",
            Self::TencentCos => "云厂商端点，使用 virtual-host 寻址；管理控制台不可用",
            Self::GenericS3 => "仅使用标准 S3 API，不启用厂商专有管理功能",
        }
    }

    /// 全部可选预设（顺序即前端展示顺序）。
    ///
    /// 阿里云 OSS / 腾讯云 COS 不再直接提供预设，需要接入云厂商时选「通用 S3」
    /// 并手动把寻址方式改为 Virtual-Host；变体保留以兼容旧配置。
    pub const fn all() -> [Self; 3] {
        [Self::Minio, Self::Rustfs, Self::GenericS3]
    }
}

/// 后端接入类型的默认参数与展示信息（供前端新建连接表单使用）。
///
/// 单一数据源在 Rust 侧，避免前后端各维护一份预设导致漂移。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackendPresetInfo {
    /// 接入类型。
    pub preset: BackendPreset,
    /// 建议的寻址方式。
    pub default_path_style: bool,
    /// 端点输入示例。
    pub endpoint_hint: String,
    /// 区域输入示例。
    pub region_hint: String,
    /// 接入须知。
    pub notes: String,
}

/// 连接档案（不含 Secret Key）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    /// 唯一标识。
    pub id: String,
    /// 展示名称。
    pub name: String,
    /// 端点 URL（含 scheme）。
    pub endpoint: String,
    /// 区域。
    pub region: String,
    /// Access Key（非机密，等同用户名，可回传前端）。
    pub access_key: String,
    /// 是否 path-style 寻址。
    pub path_style: bool,
    /// 用户选择的接入类型（决定默认参数与提示）。
    #[serde(default)]
    pub preset: BackendPreset,
    /// 探测到的后端类型。
    pub backend: Option<BackendKind>,
    /// 是否默认连接。
    pub is_default: bool,
    /// 跳过 TLS 证书校验（自签证书场景，CONN-15）。
    #[serde(default)]
    pub insecure_skip_tls_verify: bool,
    /// 自定义 CA 证书文件路径（PEM，可选）。
    #[serde(default)]
    pub custom_ca_path: Option<String>,
    /// 创建时间（RFC3339）。
    pub created_at: String,
    /// 更新时间（RFC3339）。
    pub updated_at: String,
}

/// 新建连接的输入（Secret Key 仅在创建链路中流转，绝不持久化到前端）。
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    /// 展示名称。
    pub name: String,
    /// 端点 URL。
    pub endpoint: String,
    /// 区域，留空使用默认值。
    pub region: Option<String>,
    /// Access Key。
    pub access_key: String,
    /// Secret Key（仅写入系统凭据存储，不回传）。
    pub secret_key: String,
    /// 接入类型；None 表示通用 S3。
    pub preset: Option<BackendPreset>,
    /// 是否 path-style 寻址；None 表示按接入类型取默认。
    pub path_style: Option<bool>,
    /// 跳过 TLS 证书校验（自签证书场景）。
    pub insecure_skip_tls_verify: Option<bool>,
    /// 自定义 CA 证书文件路径（PEM，可选）。
    pub custom_ca_path: Option<String>,
}

/// 更新连接的可选字段。
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionPatch {
    /// 展示名称。
    pub name: Option<String>,
    /// 端点 URL。
    pub endpoint: Option<String>,
    /// 区域。
    pub region: Option<String>,
    /// Access Key。
    pub access_key: Option<String>,
    /// Secret Key（None 表示不修改）。
    pub secret_key: Option<String>,
    /// 接入类型（None 表示不修改）。
    pub preset: Option<BackendPreset>,
    /// 是否 path-style 寻址。
    pub path_style: Option<bool>,
    /// 跳过 TLS 证书校验（None 表示不修改）。
    pub insecure_skip_tls_verify: Option<bool>,
    /// 自定义 CA 证书文件路径；空字符串表示清除。
    pub custom_ca_path: Option<String>,
}

/// 密钥状态（脱敏，开发文档 6.4）。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatus {
    /// 凭据存储中是否已配置 Secret Key。
    pub configured: bool,
    /// 脱敏后的 Access Key 标识。
    pub access_key_masked: String,
}

/// Access Key 脱敏：保留前 4 位与后 2 位。
pub fn mask_access_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    match chars.len() {
        0..=3 => "****".to_string(),
        4..=7 => {
            let head: String = chars[..4].iter().collect();
            format!("{head}****")
        }
        _ => {
            let head: String = chars[..4].iter().collect();
            let tail: String = chars[chars.len() - 2..].iter().collect();
            format!("{head}****{tail}")
        }
    }
}

/// 连接测试结果。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestOk {
    /// list_buckets 往返延迟（毫秒）。
    pub latency_ms: u32,
    /// 探测到的后端类型。
    pub backend: BackendKind,
    /// 服务端 `Server` 响应头原文（若有）。
    pub server_header: Option<String>,
    /// 可访问的桶数量。
    pub bucket_count: u32,
}

/// 桶摘要。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BucketSummary {
    /// 桶名。
    pub name: String,
    /// 创建时间（RFC3339）。
    pub created_at: Option<String>,
}

/// 对象/目录条目。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEntry {
    /// 完整 key（目录为公共前缀）。
    pub key: String,
    /// 当前层级的显示名。
    pub name: String,
    /// 是否目录（公共前缀）。
    pub is_dir: bool,
    /// 字节数（目录为 0）。
    ///
    /// Rust 侧为 `u64`，但 specta 默认禁止导出 64 位整数（防 JS 精度丢失）。
    /// S3 单对象上限 5TB ≪ 2^53，因此显式覆盖为 `number`，序列化仍是整数。
    #[specta(type = f64)]
    pub size: u64,
    /// 最后修改时间（RFC3339）。
    pub last_modified: Option<String>,
    /// ETag。
    pub etag: Option<String>,
    /// 存储类型。
    pub storage_class: Option<String>,
}

/// 对象详情（详情面板数据源，由 HEAD object 得到）。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ObjectDetail {
    /// 完整 key。
    pub key: String,
    /// 显示名（最后一段路径）。
    pub name: String,
    /// 字节数。
    #[specta(type = f64)]
    pub size: u64,
    /// 最后修改时间（RFC3339）。
    pub last_modified: Option<String>,
    /// ETag。
    pub etag: Option<String>,
    /// Content-Type（上传时按扩展名写入，后端未返回时为 None）。
    pub content_type: Option<String>,
    /// 存储类型。
    pub storage_class: Option<String>,
}

/// 对象标签（S3 Object Tagging，用于单对象多维度分类与策略管控）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ObjectTag {
    /// 标签名（最长 128 字符）。
    pub key: String,
    /// 标签值（最长 256 字符，可为空）。
    pub value: String,
}

/// 桶访问级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BucketAccess {
    /// 全员可读（已挂只读策略）。
    Public,
    /// 无策略，仅凭凭据访问。
    Private,
    /// 存在非本工具生成的自定义策略。
    Custom,
}

/// 桶策略信息。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BucketPolicyInfo {
    /// 访问级别。
    pub access: BucketAccess,
    /// 原始策略 JSON（私有为 None）。
    pub policy_json: Option<String>,
}

/// 删除桶结果。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BucketDeleteResult {
    /// 因强制删除而清空的对象数（空桶删除为 0）。
    pub removed_objects: u32,
}

/// 目录重命名结果。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RenameDirResult {
    /// 成功移动的对象数。
    pub moved: u32,
    /// 失败说明（最多 5 条）。
    pub errors: Vec<String>,
}

/// 预签名访问链接（下载 / 分享 / 预览共用）。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PresignedUrl {
    /// 可直接访问的 URL（含签名参数）。
    pub url: String,
    /// 有效期（秒）。
    pub expires_in_secs: u32,
}

/// 文本预览结果（带截断与二进制标记，避免把二进制塞进界面）。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    /// 文本内容。
    pub content: String,
    /// 是否因超过上限被截断。
    pub truncated: bool,
    /// 是否疑似二进制内容（含 NUL 字节）。
    pub binary: bool,
}

/// 下载结果。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    /// 落盘路径。
    pub path: String,
    /// 写入字节数。
    #[specta(type = f64)]
    pub size: u64,
}

/// 上传结果汇总（文件与文件夹统一返回，前端据此提示）。
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UploadSummary {
    /// 成功对象数。
    pub uploaded: u32,
    /// 失败对象数。
    pub failed: u32,
    /// 成功写入的总字节数。
    #[specta(type = f64)]
    pub total_bytes: u64,
    /// 失败明细（每项 `文件名: 原因`）。
    pub errors: Vec<String>,
}

/// 对象列表分页结果。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ObjectPage {
    /// 当前页条目。
    pub entries: Vec<ObjectEntry>,
    /// 下一页 continuation token（None 表示已到末页）。
    pub next_token: Option<String>,
}

/// 传输任务的待上传文件计划（壳层据此创建传输任务）。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlannedUpload {
    /// 本地文件绝对路径。
    pub local_path: String,
    /// 目标对象 key（含前缀与目录结构）。
    pub key: String,
    /// 文件大小（字节）。
    #[specta(type = f64)]
    pub size: u64,
}

/// 批量删除结果。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeleteResult {
    /// 实际尝试删除的对象数（目录会先展开为其下全部对象）。
    pub requested: u32,
    /// 服务端确认删除数。
    pub deleted: u32,
    /// 失败明细（`key：原因`）。
    pub errors: Vec<String>,
}

/// 后端能力集合（开发文档 2.5 能力开关）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySet {
    /// 管理控制台（用户 / Access Key / 策略 / 分组）。
    pub admin_console: bool,
    /// 桶策略。
    pub bucket_policy: bool,
    /// 生命周期规则。
    pub lifecycle: bool,
    /// 对象锁。
    pub object_lock: bool,
    /// 版本控制。
    pub versioning: bool,
    /// 预签名 URL。
    pub presigned_urls: bool,
}

impl CapabilitySet {
    /// 各后端的默认能力集合。
    ///
    /// 注意：RustFS 的取值是 P3 调研前的保守值，待管理 API 调研（ADM-R-01）后修正。
    pub fn for_backend(kind: BackendKind) -> Self {
        match kind {
            BackendKind::Minio => Self {
                admin_console: true,
                bucket_policy: true,
                lifecycle: true,
                object_lock: true,
                versioning: true,
                presigned_urls: true,
            },
            BackendKind::Rustfs => Self {
                admin_console: false,
                bucket_policy: true,
                lifecycle: false,
                object_lock: false,
                versioning: true,
                presigned_urls: true,
            },
            BackendKind::GenericS3 => Self {
                admin_console: false,
                bucket_policy: false,
                lifecycle: false,
                object_lock: false,
                versioning: false,
                presigned_urls: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_from_header() {
        assert_eq!(
            BackendKind::from_server_header("MinIO"),
            BackendKind::Minio
        );
        assert_eq!(
            BackendKind::from_server_header("RustFS/1.0"),
            BackendKind::Rustfs
        );
        assert_eq!(
            BackendKind::from_server_header("nginx"),
            BackendKind::GenericS3
        );
    }

    #[test]
    fn mask_access_key_cases() {
        assert_eq!(mask_access_key(""), "****");
        assert_eq!(mask_access_key("abc"), "****");
        assert_eq!(mask_access_key("abcd"), "abcd****");
        assert_eq!(mask_access_key("minioadmin"), "mini****in");
    }

    #[test]
    fn minio_capabilities_all_enabled() {
        let caps = CapabilitySet::for_backend(BackendKind::Minio);
        assert!(caps.admin_console && caps.lifecycle && caps.object_lock);
    }

    #[test]
    fn preset_defaults_differ_by_vendor() {
        assert!(BackendPreset::Minio.default_path_style());
        assert!(BackendPreset::Rustfs.default_path_style());
        // 云厂商走 virtual-host 寻址
        assert!(!BackendPreset::AliyunOss.default_path_style());
        assert!(!BackendPreset::TencentCos.default_path_style());
        assert_eq!(
            BackendPreset::Minio.expected_backend(),
            Some(BackendKind::Minio)
        );
        assert_eq!(BackendPreset::GenericS3.expected_backend(), None);
        assert_eq!(BackendPreset::default(), BackendPreset::GenericS3);
    }

    #[test]
    fn generic_s3_capabilities_minimal() {
        let caps = CapabilitySet::for_backend(BackendKind::GenericS3);
        assert!(!caps.admin_console && !caps.lifecycle);
        assert!(caps.presigned_urls);
    }
}
