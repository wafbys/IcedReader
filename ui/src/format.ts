/**
 * 展示层的数字格式化。书架 tooltip、编辑元数据面板、同书对照面板共用一份，
 * 免得同一本书在三处显示成不同写法。
 */

/** 文件大小：`12.3 MB` / `812 KB`；取不到（0）时给破折号，不假装是 0 字节。 */
export function fmtBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "—";
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}
