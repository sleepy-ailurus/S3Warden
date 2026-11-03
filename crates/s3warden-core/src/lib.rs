//! S3Warden 核心库。
//!
//! 承载全部业务逻辑：S3 操作、连接与配置管理、密钥存储、后端探测、能力模型。
//! 本 crate **不依赖 Tauri**，可独立构建与测试；Tauri 壳只做 command 薄封装（见开发文档 4.2/4.3）。
//!
//! 安全红线（开发文档 2.6 / 6.4）：
//! - Secret Key 只经 [`secrets`] 模块进出操作系统凭据存储；
//! - 对外返回的类型（[`types`]）不含任何 Secret 字段。

pub mod api;
pub mod config;
pub mod error;
pub mod probe;
pub mod s3;
pub mod secrets;
pub mod types;

pub use error::{AppError, AppResult};
