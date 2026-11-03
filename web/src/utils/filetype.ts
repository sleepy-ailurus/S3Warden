/** 文件类型判定：驱动详情面板图标、预览方式与预览可用性。 */

const IMAGE_EXTS = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "bmp",
  "ico",
  "svg",
  "avif",
]);

const TEXT_EXTS = new Set([
  "txt",
  "log",
  "md",
  "markdown",
  "json",
  "xml",
  "yaml",
  "yml",
  "toml",
  "csv",
  "html",
  "htm",
  "css",
  "js",
  "mjs",
  "ts",
  "tsx",
  "vue",
  "rs",
  "go",
  "py",
  "java",
  "sh",
  "sql",
  "ini",
  "conf",
]);

const VIDEO_EXTS = new Set(["mp4", "mov", "webm", "mkv", "avi"]);
const AUDIO_EXTS = new Set(["mp3", "wav", "flac", "ogg", "m4a"]);
const PDF_EXTS = new Set(["pdf"]);

/** 取小写扩展名（无扩展名返回空串）。 */
export function extOf(name: string): string {
  const i = name.lastIndexOf(".");
  if (i <= 0 || i === name.length - 1) return "";
  return name.slice(i + 1).toLowerCase();
}

/** 是否可用 <img> 直接预览。 */
export function isImage(name: string): boolean {
  return IMAGE_EXTS.has(extOf(name));
}

/** 是否按文本预览。 */
export function isText(name: string): boolean {
  return TEXT_EXTS.has(extOf(name));
}

/** 是否可用浏览器内嵌播放器预览。 */
export function isMedia(name: string): boolean {
  const e = extOf(name);
  return VIDEO_EXTS.has(e) || AUDIO_EXTS.has(e) || PDF_EXTS.has(e);
}

/** 是否支持任意形式的预览。 */
export function isPreviewable(name: string): boolean {
  return isImage(name) || isText(name) || isMedia(name);
}

/** 文件类型中文标签（详情面板「类型」属性行）。 */
export function typeLabel(name: string): string {
  const e = extOf(name);
  if (!e) return "文件";
  if (IMAGE_EXTS.has(e)) return "图片";
  if (VIDEO_EXTS.has(e)) return "视频";
  if (AUDIO_EXTS.has(e)) return "音频";
  if (PDF_EXTS.has(e)) return "PDF";
  if (TEXT_EXTS.has(e)) return "文本";
  return e.toUpperCase();
}
