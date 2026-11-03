/**
 * IPC 结果解包。
 *
 * tauri-specta 2 把 Rust 的 `Result<T, AppError>` 映射为判别联合
 * `{ status: "ok"; data: T } | { status: "error"; error: AppError }`，不会自动抛异常。
 * 本模块把它统一转成「成功返回值 / 失败抛 IpcError」，让业务代码保持 async/await + try/catch 风格，
 * 同时保留结构化错误（kind + message）供前端分类型提示（开发文档 6.2）。
 */

import type { AppError } from "../bindings";

/** 生成的命令返回类型。 */
export type IpcResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: AppError };

/** 业务异常：携带 Rust 侧的结构化错误。 */
export class IpcError extends Error {
  /** 错误类别，来自 AppError 判别联合。 */
  readonly kind: AppError["kind"];

  constructor(readonly detail: AppError) {
    // 单元变体（如 cancelled）序列化后不含 message 字段
    const msg = (detail as { message?: string }).message ?? "操作已取消";
    super(msg);
    this.name = "IpcError";
    this.kind = detail.kind;
  }
}

/** 解包 IPC 结果：成功返回数据，失败抛 {@link IpcError}。 */
export function unwrap<T>(result: IpcResult<T>): T {
  if (result.status === "ok") return result.data;
  throw new IpcError(result.error);
}
