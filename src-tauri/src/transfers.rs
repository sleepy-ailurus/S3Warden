//! 传输任务管理：注册表、并发排队、进度/状态事件推送（XFER 组）。
//!
//! 职责边界：真正的传输逻辑在 `s3warden-core::transfer`；本模块只做
//! 任务编排（并发上限、取消、重试）与 IPC 事件推送。Secret Key 仍只经
//! 核心层 `api::prepare_transfer` 流转，壳层拿不到明文。

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use s3warden_core::api;
use s3warden_core::error::AppError;
use s3warden_core::transfer::{self, CancelToken};

/// 同时运行的任务数上限（其余排队）。
const MAX_CONCURRENT: usize = 3;

/// 进度事件推送间隔（毫秒）。
const PROGRESS_EMIT_MS: u64 = 150;

// ---------------------------------------------------------------------------
// 事件（tauri-specta 类型化事件，前端经 bindings.events.* 监听）
// ---------------------------------------------------------------------------

/// 传输进度事件（节流推送）。
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgressEvent {
    /// 任务 ID。
    pub task_id: String,
    /// 已传输字节。
    #[specta(type = f64)]
    pub transferred: u64,
    /// 总字节（未知为 0）。
    #[specta(type = f64)]
    pub total: u64,
}

/// 传输状态事件。
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatusEvent {
    /// 任务 ID。
    pub task_id: String,
    /// queued / running / completed / failed / cancelled。
    pub status: String,
    /// 失败原因（仅 failed）。
    pub error: Option<String>,
    #[specta(type = f64)]
    pub transferred: u64,
    #[specta(type = f64)]
    pub total: u64,
}

// ---------------------------------------------------------------------------
// 任务模型
// ---------------------------------------------------------------------------

/// 传输方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    /// 上传。
    Upload,
    /// 下载。
    Download,
}

/// 任务参数（重试时复用）。
#[derive(Debug, Clone)]
struct TransferParams {
    kind: TransferKind,
    conn_id: String,
    bucket: String,
    /// 上传 = 目标 key；下载 = 源 key。
    key: String,
    /// 上传 = 本地源文件；下载 = 本地目标路径。
    local_path: String,
    total: u64,
    /// 展示名（文件名）。
    display: String,
}

/// 新建任务返回给前端的信息。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TransferTaskInfo {
    pub task_id: String,
    pub kind: TransferKind,
    pub name: String,
    pub key: String,
    pub local_path: String,
    #[specta(type = f64)]
    pub total: u64,
}

struct TaskEntry {
    cancel: Arc<AtomicBool>,
    params: TransferParams,
}

#[derive(Default)]
struct RegistryInner {
    tasks: HashMap<String, TaskEntry>,
    queue: VecDeque<String>,
    running: usize,
}

/// 传输任务注册表（Tauri managed state）。
#[derive(Default)]
pub struct TransferRegistry {
    inner: Mutex<RegistryInner>,
}

impl TransferRegistry {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RegistryInner>, AppError> {
        self.inner
            .lock()
            .map_err(|_| AppError::Config("传输注册表已损坏".to_string()))
    }
}

fn status_event(task_id: &str, status: &str, error: Option<String>, transferred: u64, total: u64) -> TransferStatusEvent {
    TransferStatusEvent {
        task_id: task_id.to_string(),
        status: status.to_string(),
        error,
        transferred,
        total,
    }
}

/// 入队一个任务并尝试启动。
fn enqueue(
    app: &AppHandle,
    reg: &TransferRegistry,
    params: TransferParams,
) -> Result<TransferTaskInfo, AppError> {
    let task_id = uuid::Uuid::new_v4().to_string();
    let info = TransferTaskInfo {
        task_id: task_id.clone(),
        kind: params.kind,
        name: params.display.clone(),
        key: params.key.clone(),
        local_path: params.local_path.clone(),
        total: params.total,
    };

    {
        let mut g = reg.lock()?;
        g.tasks.insert(
            task_id.clone(),
            TaskEntry {
                cancel: Arc::new(AtomicBool::new(false)),
                params,
            },
        );
        g.queue.push_back(task_id.clone());
    }
    let _ = status_event(&task_id, "queued", None, 0, info.total).emit(app);
    pump(app, reg)?;
    Ok(info)
}

/// 从队列补位启动任务（并发未满时）。
fn pump(app: &AppHandle, reg: &TransferRegistry) -> Result<(), AppError> {
    loop {
        let mut next: Option<(String, TransferParams, CancelToken)> = None;
        {
            let mut g = reg.lock()?;
            while g.running < MAX_CONCURRENT {
                let Some(task_id) = g.queue.pop_front() else { break };
                let Some(entry) = g.tasks.get(&task_id) else { continue };
                // 排队期间被取消的任务直接丢弃
                if entry.cancel.load(Ordering::Relaxed) {
                    let total = entry.params.total;
                    let _ = status_event(&task_id, "cancelled", None, 0, total).emit(app);
                    continue;
                }
                // 先结束不可变借用再改 running
                let cloned = (task_id.clone(), entry.params.clone(), CancelToken(entry.cancel.clone()));
                let (task_id, params, cancel) = cloned;
                g.running += 1;
                next = Some((task_id, params, cancel));
                break;
            }
        }
        let Some((task_id, params, cancel)) = next else { break };
        spawn_task(app.clone(), task_id, params, cancel);
    }
    Ok(())
}

/// 启动一个后台传输任务。
fn spawn_task(app: AppHandle, task_id: String, params: TransferParams, cancel: CancelToken) {
    tauri::async_runtime::spawn(async move {
        let total = params.total;
        let _ = status_event(&task_id, "running", None, 0, total).emit(&app);

        // 进度节流闭包（核心层回调字节计数，这里转事件推送）。
        let app_for_progress = app.clone();
        let task_for_progress = task_id.clone();
        let last_emit = Mutex::new(Instant::now() - Duration::from_millis(PROGRESS_EMIT_MS));
        let progress = move |done: u64| {
            let mut last = match last_emit.lock() {
                Ok(l) => l,
                Err(_) => return,
            };
            if last.elapsed().as_millis() >= u128::from(PROGRESS_EMIT_MS) {
                *last = Instant::now();
                drop(last);
                let _ = TransferProgressEvent {
                    task_id: task_for_progress.clone(),
                    transferred: done,
                    total,
                }
                .emit(&app_for_progress);
            }
        };

        let result = run_transfer(&params, &cancel, &progress).await;

        let (status, error, transferred) = match &result {
            Ok(n) => ("completed", None, *n),
            Err(AppError::Cancelled) => ("cancelled", None, 0),
            Err(e) => ("failed", Some(e.to_string()), 0),
        };
        let _ = status_event(&task_id, status, error, transferred, total).emit(&app);

        // 收尾：让出并发名额并补位。
        if let Some(reg) = app.try_state::<TransferRegistry>() {
            if let Ok(mut g) = reg.lock() {
                g.running = g.running.saturating_sub(1);
            }
            let _ = pump(&app, &reg);
        }
    });
}

/// 执行单个传输任务（上传或下载）。
async fn run_transfer(
    params: &TransferParams,
    cancel: &CancelToken,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> s3warden_core::AppResult<u64> {
    let state_dir = api::transfer_state_dir()?;
    let (_profile, client) = api::prepare_transfer(&params.conn_id).await?;

    match params.kind {
        TransferKind::Upload => {
            transfer::upload_file(
                &client,
                &params.conn_id,
                &params.bucket,
                &params.key,
                Path::new(&params.local_path),
                &state_dir,
                progress,
                cancel,
            )
            .await
        }
        TransferKind::Download => {
            transfer::download_to_file(
                &client,
                &params.bucket,
                &params.key,
                Path::new(&params.local_path),
                progress,
                cancel,
            )
            .await
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// 批量上传：展开文件/文件夹后每文件一个任务入队。
#[tauri::command]
#[specta::specta]
pub async fn start_upload(
    app: AppHandle,
    id: String,
    bucket: String,
    prefix: String,
    paths: Vec<String>,
) -> Result<Vec<TransferTaskInfo>, AppError> {
    if bucket.is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    let plans = api::collect_upload_plans(&paths, &prefix)?;
    let reg = app.state::<TransferRegistry>();
    let mut infos = Vec::with_capacity(plans.len());
    for p in plans {
        let display = p.key.rsplit('/').next().unwrap_or(&p.key).to_string();
        let params = TransferParams {
            kind: TransferKind::Upload,
            conn_id: id.clone(),
            bucket: bucket.clone(),
            key: p.key,
            local_path: p.local_path,
            total: p.size,
            display,
        };
        infos.push(enqueue(&app, &reg, params)?);
    }
    Ok(infos)
}

/// 下载对象到本地路径（入传输队列）。
#[tauri::command]
#[specta::specta]
pub async fn start_download(
    app: AppHandle,
    id: String,
    bucket: String,
    key: String,
    dest_path: String,
) -> Result<TransferTaskInfo, AppError> {
    if bucket.is_empty() {
        return Err(AppError::Invalid("bucket 不能为空".to_string()));
    }
    if key.trim().is_empty() || dest_path.trim().is_empty() {
        return Err(AppError::Invalid("下载参数不完整".to_string()));
    }
    // 总大小取元数据（失败不阻塞，total 记 0）。
    let total = api::stat_object(&id, &bucket, &key)
        .await
        .map(|d| d.size)
        .unwrap_or(0);
    let display = key.rsplit('/').next().unwrap_or("下载").to_string();
    let params = TransferParams {
        kind: TransferKind::Download,
        conn_id: id,
        bucket,
        key,
        local_path: dest_path,
        total,
        display,
    };
    let reg = app.state::<TransferRegistry>();
    enqueue(&app, &reg, params)
}

/// 取消任务（排队中直接丢弃；运行中尽快中止并清理临时状态）。
#[tauri::command]
#[specta::specta]
pub fn cancel_transfer(app: AppHandle, task_id: String) -> Result<(), AppError> {
    let reg = app.state::<TransferRegistry>();
    let g = reg.lock()?;
    let Some(entry) = g.tasks.get(&task_id) else {
        return Err(AppError::NotFound("传输任务不存在".to_string()));
    };
    entry.cancel.store(true, Ordering::Relaxed);
    Ok(())
}

/// 重试失败/取消的任务（上传自动从已传分片续传，下载从断点继续）。
#[tauri::command]
#[specta::specta]
pub fn retry_transfer(app: AppHandle, task_id: String) -> Result<(), AppError> {
    let reg = app.state::<TransferRegistry>();
    let total = {
        let mut g = reg.lock()?;
        let Some(entry) = g.tasks.get_mut(&task_id) else {
            return Err(AppError::NotFound("传输任务不存在".to_string()));
        };
        entry.cancel.store(false, Ordering::Relaxed);
        let total = entry.params.total;
        g.queue.push_back(task_id.clone());
        total
    };
    let _ = status_event(&task_id, "queued", None, 0, total).emit(&app);
    let _ = pump(&app, &reg);
    Ok(())
}
