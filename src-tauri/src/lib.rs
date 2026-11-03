//! S3Warden Tauri 壳：只做 command 注册与转发，不含业务逻辑（开发文档 4.2）。
//!
//! 业务实现全部在 `s3warden-core`；本 crate 只提供类型安全的 IPC 入口。

mod commands;
mod transfers;

use specta_typescript::Typescript;
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder};

/// 生成的 TypeScript 绑定路径（基于 manifest 目录，与 CWD 无关）。
const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/src/bindings.ts");

/// tauri-specta Builder：commands 与 events 的唯一注册处。
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::list_backend_presets,
            commands::create_connection,
            commands::list_connections,
            commands::update_connection,
            commands::delete_connection,
            commands::set_default_connection,
            commands::secret_status,
            commands::reveal_secret,
            commands::test_new_connection,
            commands::test_saved_connection,
            commands::list_buckets,
            commands::create_bucket,
            commands::delete_bucket,
            commands::list_objects,
            commands::stat_object,
            commands::presign_object,
            commands::read_object_text,
            commands::download_object,
            commands::delete_object,
            commands::rename_object,
            commands::rename_directory,
            commands::get_object_tags,
            commands::put_object_tags,
            commands::get_bucket_access,
            commands::set_bucket_public,
            commands::upload_paths,
            commands::create_prefix,
            commands::delete_objects_batch,
            transfers::start_upload,
            transfers::start_download,
            transfers::cancel_transfer,
            transfers::retry_transfer,
        ])
        .events(collect_events![
            transfers::TransferProgressEvent,
            transfers::TransferStatusEvent,
        ])
}

/// 导出 TypeScript 绑定。
///
/// 两个调用方共用：debug 启动时自动导出；CI 中经 `export-bindings` bin 导出后做漂移检查。
pub fn export_bindings() {
    let ts = Typescript::default();
    specta_builder()
        .export(ts, BINDINGS_PATH)
        .expect("导出 TypeScript 绑定失败");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    export_bindings();

    let builder = specta_builder();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(transfers::TransferRegistry::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            // 系统为深色模式时 Windows 原生标题栏会变黑，与浅色界面割裂；
            // 显式指定浅色主题，保持整窗口色调一致。
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_theme(Some(tauri::Theme::Light));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
