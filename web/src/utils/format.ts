/** 字节数人性化显示。 */
export function formatSize(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "—";
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  const num = v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1);
  return `${num} ${units[i]}`;
}

/** RFC3339 → 本地时间。 */
export function formatTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

/** 从 URL 提取 host:port。 */
export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** 后端类型显示名。 */
export function backendLabel(b: "minio" | "rustfs" | "generic_s3"): string {
  if (b === "minio") return "MinIO";
  if (b === "rustfs") return "RustFS";
  return "S3 兼容";
}

/** 后端类型颜色（连接列表圆点）。 */
export function backendColor(b: "minio" | "rustfs" | "generic_s3" | null | undefined): string {
  if (b === "minio") return "#18a058";
  if (b === "rustfs") return "#f0a020";
  return "#909399";
}

/** 接入类型显示名（本地化放在前端，Rust 只提供枚举值）。 */
export function presetLabel(p: string | null | undefined): string {
  switch (p) {
    case "minio":
      return "MinIO";
    case "rustfs":
      return "RustFS";
    case "aliyun_oss":
      return "阿里云 OSS";
    case "tencent_cos":
      return "腾讯云 COS";
    default:
      return "通用 S3";
  }
}

/** 接入类型颜色（连接列表圆点，用用户选择的预设而非探测结果，更稳定）。 */
export function presetColor(p: string | null | undefined): string {
  switch (p) {
    case "minio":
      return "#18a058";
    case "rustfs":
      return "#f0a020";
    case "aliyun_oss":
      return "#ff6a00";
    case "tencent_cos":
      return "#0052d9";
    default:
      return "#909399";
  }
}

/** 把 IPC 错误（AppError 判别联合）转成可读文本。 */
export function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return JSON.stringify(e);
}
