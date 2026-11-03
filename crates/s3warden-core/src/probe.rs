//! 后端类型探测。
//!
//! 通过 HTTP `Server` 响应头识别 MinIO / RustFS / 通用 S3（CONN-12）。
//! 探测失败不报错，一律回退 [`BackendKind::GenericS3`]——连接是否可用由 list_buckets 探活决定。

use crate::s3::TlsOptions;
use crate::types::BackendKind;

/// 探测超时（毫秒）。
const PROBE_TIMEOUT_MS: u64 = 5_000;

/// 对端点发起 HEAD 请求，依据 `Server` 头识别后端。
///
/// 自签证书 / 自定义 CA 场景需传入与连接一致的 [`TlsOptions`]，否则探测会失败。
pub async fn probe_backend(endpoint: &str, tls: &TlsOptions) -> (BackendKind, Option<String>) {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(PROBE_TIMEOUT_MS));
    if tls.insecure_skip_verify {
        builder = builder.danger_accept_invalid_certs(true);
    }
    if let Some(pem) = &tls.custom_ca_pem {
        if let Ok(cert) = reqwest::Certificate::from_pem(pem) {
            builder = builder.add_root_certificate(cert);
        }
    }
    let Ok(client) = builder.build() else {
        return (BackendKind::GenericS3, None);
    };

    match client.head(endpoint).send().await {
        Ok(resp) => {
            let server = resp
                .headers()
                .get(reqwest::header::SERVER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let kind = server
                .as_deref()
                .map(BackendKind::from_server_header)
                .unwrap_or(BackendKind::GenericS3);
            (kind, server)
        }
        Err(_) => (BackendKind::GenericS3, None),
    }
}
