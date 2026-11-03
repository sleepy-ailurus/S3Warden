//! 管理 API 适配层（开发文档 6.5）。
//!
//! 里程碑：P2 实现 MinIO 适配，P3 实现 RustFS 适配。
//! P0 阶段仅固化契约——统一由 [`AdminBackend`] 抽象各后端管理能力，
//! 前端只依赖 [`s3warden_core::types::CapabilitySet`] 做功能显隐，
//! 新增后端 = 新增一个本 trait 的实现，不改变前端调用约定。

use s3warden_core::types::{BackendKind, CapabilitySet};

/// 各后端管理 API 的统一抽象。
pub trait AdminBackend: Send + Sync {
    /// 后端类型标识。
    fn kind(&self) -> BackendKind;

    /// 该后端支持的能力集合。
    fn capabilities(&self) -> CapabilitySet;

    /// 管理接口当前是否可用（例如是否具备管理员凭据）。
    fn admin_available(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MinioStub;

    impl AdminBackend for MinioStub {
        fn kind(&self) -> BackendKind {
            BackendKind::Minio
        }
        fn capabilities(&self) -> CapabilitySet {
            CapabilitySet::for_backend(BackendKind::Minio)
        }
    }

    #[test]
    fn trait_contract_minio() {
        let stub = MinioStub;
        assert_eq!(stub.kind(), BackendKind::Minio);
        assert!(stub.capabilities().admin_console);
        assert!(!stub.admin_available());
    }
}
