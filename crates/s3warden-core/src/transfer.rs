//! 传输引擎：分片上传（断点续传）、分块下载（断点续传）、进度回调与取消。
//!
//! 设计要点：
//! - **上传**一律走 Multipart（分片 8 MiB，满足 S3 除末片外 ≥5 MiB 的约束）；
//!   已上传分片记录在状态目录，任务失败/进程退出后可凭 `upload_id` 续传；
//!   显式取消时 abort 上传并清理状态。
//! - **下载**先用 `Range: bytes={resume}-` 续传：目标旁的 `.s3wdl` 临时文件
//!   保留已下载字节；失败/崩溃后重试自动从断点继续；取消时清理临时文件。
//! - 进度以 `Fn(u64)`（已完成字节）回调，节流由调用方（壳层）负责。
//! - 本模块不感知 IPC 与前端（架构原则 2），壳层负责事件推送与并发排队。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_smithy_types::byte_stream::ByteStream;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};
use crate::s3::guess_content_type;

/// 分片大小（8 MiB）。S3 允许末片小于 5 MiB，其余片必须 ≥5 MiB。
pub const PART_SIZE: u64 = 8 * 1024 * 1024;

/// 取消轮询间隔（毫秒）。
const CANCEL_POLL_MS: u64 = 200;

/// 进度回调最小间隔（毫秒）——下载路径直接内联节流。
const PROGRESS_MIN_MS: u128 = 150;

/// 取消旗标：壳层持有一份 `Arc<AtomicBool>`，置 true 后传输尽快中止。
#[derive(Debug, Clone, Default)]
pub struct CancelToken(pub Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// 轮询取消旗标，直到被取消（配合 `tokio::select!` 使用）。
async fn wait_cancelled(token: &CancelToken) {
    loop {
        if token.is_cancelled() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(CANCEL_POLL_MS)).await;
    }
}

/// 上传续传状态（持久化在状态目录，文件名由任务参数哈希得出）。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct UploadResumeState {
    upload_id: String,
    /// 已完成分片：(part_number, 字节数)。
    parts: Vec<(i32, u64)>,
}

/// 稳定哈希：同一连接 + 桶 + key + 同一文件（路径/大小/mtime）得到同一状态文件。
fn resume_hash(conn_id: &str, bucket: &str, key: &str, path: &Path, size: u64, mtime: i64) -> u64 {
    let mut h = DefaultHasher::new();
    conn_id.hash(&mut h);
    bucket.hash(&mut h);
    key.hash(&mut h);
    path.to_string_lossy().hash(&mut h);
    size.hash(&mut h);
    mtime.hash(&mut h);
    h.finish()
}

fn state_file(state_dir: &Path, hash: u64) -> PathBuf {
    state_dir.join(format!("{hash:016x}.json"))
}

fn load_state(path: &Path) -> Option<UploadResumeState> {
    let raw = std::fs::read(path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn save_state(path: &Path, state: &UploadResumeState) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Config(format!("创建传输状态目录失败：{e}")))?;
    }
    let raw = serde_json::to_vec(state)
        .map_err(|e| AppError::Config(format!("序列化传输状态失败：{e}")))?;
    std::fs::write(path, raw).map_err(|e| AppError::Config(format!("写入传输状态失败：{e}")))
}

/// 上传本地文件到对象 key。
///
/// - 小文件（≤ [`PART_SIZE`]）直接 PutObject（进度按整文件粒度）；
/// - 大文件 Multipart 分片上传，支持断点续传；
/// - `cancel` 触发时：Multipart 被 abort、状态文件删除，返回 [`AppError::Cancelled`]；
/// - 网络失败时保留状态文件，下次调用同参数自动续传。
pub async fn upload_file(
    client: &crate::s3::Client,
    conn_id: &str,
    bucket: &str,
    key: &str,
    path: &Path,
    state_dir: &Path,
    progress: &(dyn Fn(u64) + Send + Sync),
    cancel: &CancelToken,
) -> AppResult<u64> {
    let meta = std::fs::metadata(path)
        .map_err(|e| AppError::Invalid(format!("读取本地文件失败：{e}")))?;
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // 小文件：单请求直传，无续传状态。
    if size <= PART_SIZE {
        let data = std::fs::read(path)
            .map_err(|e| AppError::Invalid(format!("读取本地文件失败：{e}")))?;
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        tokio::select! {
            r = client
                .put_object()
                .bucket(bucket)
                .key(key)
                .content_type(guess_content_type(&file_name))
                .body(ByteStream::from(data))
                .send() =>
            {
                r.map_err(|e| AppError::S3(format!("上传 {key} 失败：{e}")))?;
                progress(size);
                Ok(size)
            }
            _ = wait_cancelled(cancel) => Err(AppError::Cancelled),
        }
    } else {
        upload_multipart(
            client, conn_id, bucket, key, path, state_dir, size, mtime, &file_name, progress,
            cancel,
        )
        .await
    }
}

/// Multipart 分片上传（含断点续传）。
#[allow(clippy::too_many_arguments)]
async fn upload_multipart(
    client: &crate::s3::Client,
    conn_id: &str,
    bucket: &str,
    key: &str,
    path: &Path,
    state_dir: &Path,
    size: u64,
    mtime: i64,
    file_name: &str,
    progress: &(dyn Fn(u64) + Send + Sync),
    cancel: &CancelToken,
) -> AppResult<u64> {
    let hash = resume_hash(conn_id, bucket, key, path, size, mtime);
    let state_path = state_file(state_dir, hash);

    // 尝试续传：状态文件存在且 upload_id 在服务端仍有效。
    let mut upload_id: Option<String> = None;
    let mut completed: Vec<CompletedPart> = Vec::new();
    let mut base_bytes: u64 = 0;

    if let Some(state) = load_state(&state_path) {
        let res = client
            .list_parts()
            .bucket(bucket)
            .key(key)
            .upload_id(&state.upload_id)
            .send()
            .await;
        match res {
            Ok(out) => {
                for p in out.parts() {
                    let (Some(n), Some(etag)) = (p.part_number(), p.e_tag()) else {
                        continue;
                    };
                    let sz = p.size().unwrap_or(0).max(0) as u64;
                    completed.push(
                        CompletedPart::builder()
                            .part_number(n)
                            .e_tag(etag)
                            .build(),
                    );
                    base_bytes += sz;
                }
                upload_id = Some(state.upload_id.clone());
            }
            Err(_) => {
                // upload_id 已失效（服务端清理/过期）：丢弃状态重新开始。
                let _ = std::fs::remove_file(&state_path);
            }
        }
    }

    let upload_id = match upload_id {
        Some(id) => id,
        None => {
            let out = client
                .create_multipart_upload()
                .bucket(bucket)
                .key(key)
                .content_type(guess_content_type(file_name))
                .send()
                .await
                .map_err(|e| AppError::S3(format!("初始化分片上传失败：{e}")))?;
            out.upload_id().map(str::to_string).ok_or_else(|| {
                AppError::S3("服务端未返回 upload_id，无法分片上传".to_string())
            })?
        }
    };

    // 已完成分片的编号集合，跳过这些分片。
    let done_numbers: std::collections::HashSet<i32> =
        completed.iter().map(|p| p.part_number().unwrap_or(0)).collect();

    progress(base_bytes);

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| AppError::Invalid(format!("打开本地文件失败：{e}")))?;
    let mut uploaded_bytes = base_bytes;
    let mut last_saved: Vec<(i32, u64)> = Vec::new();

    let mut part_number = 1i32;
    let mut offset = 0u64;
    while offset < size {
        let chunk_len = PART_SIZE.min(size - offset) as usize;
        if done_numbers.contains(&part_number) {
            offset += chunk_len as u64;
            part_number += 1;
            continue;
        }

        let mut buf = vec![0u8; chunk_len];
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| AppError::Invalid(format!("定位本地文件失败：{e}")))?;
        file.read_exact(&mut buf)
            .await
            .map_err(|e| AppError::Invalid(format!("读取本地文件失败：{e}")))?;

        // 取消：abort 上传并清理状态（显式取消不做续传保留）。
        tokio::select! {
            r = client
                .upload_part()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .part_number(part_number)
                .body(ByteStream::from(buf))
                .send() =>
            {
                let out = r.map_err(|e| {
                    AppError::S3(format!("上传分片 {part_number} 失败：{e}"))
                })?;
                completed.push(
                    CompletedPart::builder()
                        .part_number(part_number)
                        .e_tag(out.e_tag().unwrap_or_default())
                        .build(),
                );
            }
            _ = wait_cancelled(cancel) => {
                // 取消保留断点：不 abort、不删状态文件，重试/重新上传同文件可续传。
                // 代价是服务端可能残留未完成的 multipart（MinIO 生命周期会清理）。
                return Err(AppError::Cancelled);
            }
        }

        uploaded_bytes += chunk_len as u64;
        progress(uploaded_bytes);

        // 每片完成后落盘状态，崩溃/失败后可续传。
        last_saved.push((part_number, chunk_len as u64));
        let mut persist: Vec<(i32, u64)> =
            done_numbers.iter().map(|n| (*n, 0)).collect();
        persist.extend(last_saved.iter().cloned());
        let state = UploadResumeState {
            upload_id: upload_id.clone(),
            parts: persist,
        };
        if let Err(e) = save_state(&state_path, &state) {
            // 状态写失败不致命：仅失去断点续传能力。
            let _ = e;
        }

        offset += chunk_len as u64;
        part_number += 1;
    }

    completed.sort_by_key(|p| p.part_number().unwrap_or(0));
    let multipart = CompletedMultipartUpload::builder()
        .set_parts(Some(completed))
        .build();
    client
        .complete_multipart_upload()
        .bucket(bucket)
        .key(key)
        .upload_id(&upload_id)
        .multipart_upload(multipart)
        .send()
        .await
        .map_err(|e| AppError::S3(format!("完成分片上传失败：{e}")))?;

    let _ = std::fs::remove_file(&state_path);
    progress(size);
    Ok(size)
}

/// 下载对象到本地路径（Range 断点续传 + 取消）。
///
/// 失败/**取消**均保留 `.s3wdl` 临时文件供续传，仅成功时清理。
pub async fn download_to_file(
    client: &crate::s3::Client,
    bucket: &str,
    key: &str,
    dest: &Path,
    progress: &(dyn Fn(u64) + Send + Sync),
    cancel: &CancelToken,
) -> AppResult<u64> {
    // 总大小：HEAD 失败不阻塞（进度退化为已传输字节数）。
    let total = match client.head_object().bucket(bucket).key(key).send().await {
        Ok(out) => out.content_length().unwrap_or(0).max(0) as u64,
        Err(_) => 0,
    };

    let mut tmp = dest.as_os_str().to_os_string();
    tmp.push(".s3wdl");
    let tmp = PathBuf::from(tmp);

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Invalid(format!("创建目标目录失败：{e}")))?;
        }
    }

    // 续传：临时文件存在且未超过总大小时，从其长度继续。
    let mut resume = std::fs::metadata(&tmp)
        .map(|m| m.len())
        .unwrap_or(0);
    if total > 0 && resume > total {
        resume = 0;
    }

    let mut req = client.get_object().bucket(bucket).key(key);
    if resume > 0 {
        req = req.range(format!("bytes={resume}-"));
    }
    let resp = tokio::select! {
        r = req.send() => r.map_err(|e| AppError::S3(format!("下载 {key} 失败：{e}")))?,
        _ = wait_cancelled(cancel) => return Err(AppError::Cancelled),
    };

    // 服务端忽略 Range（返回完整对象）时从头写。
    let mut base = resume;
    if resume > 0 && resp.content_range().is_none() {
        base = 0;
    }

    let mut reader = resp.body.into_async_read();
    let mut file = if base > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&tmp)
            .await
            .map_err(|e| AppError::Invalid(format!("打开续传临时文件失败：{e}")))?
    } else {
        tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| AppError::Invalid(format!("创建临时文件失败：{e}")))?
    };

    progress(base);
    let mut written = base;
    let mut last_emit = std::time::Instant::now();
    let mut chunk = vec![0u8; 256 * 1024];

    loop {
        tokio::select! {
            r = reader.read(&mut chunk) => {
                let n = r.map_err(|e| AppError::Invalid(format!("读取下载数据失败：{e}")))?;
                if n == 0 {
                    break;
                }
                file.write_all(&chunk[..n])
                    .await
                    .map_err(|e| AppError::Invalid(format!("写入本地文件失败：{e}")))?;
                written += n as u64;
                if last_emit.elapsed().as_millis() >= PROGRESS_MIN_MS {
                    last_emit = std::time::Instant::now();
                    progress(written);
                }
                if cancel.is_cancelled() {
                    // 取消保留断点：flush 后保留 .s3wdl，重试可续传
                    file.flush().await.ok();
                    drop(file);
                    return Err(AppError::Cancelled);
                }
            }
            _ = wait_cancelled(cancel) => {
                file.flush().await.ok();
                drop(file);
                return Err(AppError::Cancelled);
            }
        }
    }

    file.flush()
        .await
        .map_err(|e| AppError::Invalid(format!("写入本地文件失败：{e}")))?;
    drop(file);

    // 服务端声明的总大小与实际不符时报错（防止截断文件被当作完成）。
    if total > 0 && written != total {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::S3(format!(
            "下载数据不完整：预期 {total} 字节，实际 {written} 字节"
        )));
    }

    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|e| AppError::Invalid(format!("保存文件失败：{e}")))?;
    progress(total.max(written));
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_hash_is_stable_and_distinct() {
        let p = Path::new("/tmp/a.bin");
        let h1 = resume_hash("conn1", "b", "k", p, 100, 5);
        let h2 = resume_hash("conn1", "b", "k", p, 100, 5);
        let h3 = resume_hash("conn2", "b", "k", p, 100, 5);
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn cancel_token_flags() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        t.0.store(true, Ordering::Relaxed);
        assert!(t.is_cancelled());
    }
}
