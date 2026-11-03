//! 后端类型探测。
//!
//! 通过 HTTP `Server` 响应头识别 MinIO / RustFS / 通用 S3（CONN-12）。
//! 探测失败不报错，一律回退 [`BackendKind::GenericS3`]——连接是否可用由 list_buckets 探活决定。

use crate::types::BackendKind;

/// 探测超时（毫秒）。
const PROBE_TIMEOUT_MS: u64 = 5_000;

/// 对端点发起 HEAD 请求，依据 `Server` 头识别后端。
pub async fn probe_backend(endpoint: &str) -> (BackendKind, Option<String>) {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(PROBE_TIMEOUT_MS))
        .build()
    else {
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
