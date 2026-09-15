/**
 * PDF 连续纸带的模型层——纯函数，`PdfView.tsx` 只负责画与滚动。
 *
 * 这里**没有**「页窗口」概念：SumatraPDF 式阅读面是一条纵向连续的纸带，一「行」放
 * 一张或两张纸，缩放随窗口实时重算，滚动就是滚动。Rust 那边一页仍是一个 spine 单元、
 * 仍是 `page/NNNN.webp?w=` 的资源契约，前端只拼 URL、不解析 PDF。
 *
 * 三件事在这里定死，方便读也方便改：
 *
 * - **配对**（[`pageRows`]）：封面（第 1 页）单独一行，之后 2-3、4-5…；单页时一行一张。
 * - **一张纸画多大**（[`sheetSize`]）：适应宽度由宽度定、适应页面由高度定；双页各占
 *   一半（适应页面双页放不下时按比例缩到放得下，免得横向裁掉）。
 * - **自动双页**（[`autoSpread`]）：用**真实页面宽高比**判，不用窗口比例猜——横向页
 *   自然落回单页。
 *
 * 页号一律 1-based（与 `spine` href、资源 URL 一致）；`index` 是 App 里的 0-based 下标。
 */

/** 缩放：适应页面（纸高＝阅读区高）/ 适应宽度（纸宽＝阅读区宽）。 */
export type PdfFit = "page" | "width";
/** 用户设置：自动 / 单页 / 双页。 */
export type PdfSpread = "auto" | "single" | "double";
/** 判定之后的张数——版式代码只认这两种（[`resolveSpread`]）。 */
export type PdfResolvedSpread = "single" | "double";

/** 两张纸之间的中缝（CSS px）。并排判据与排版共用这一个值。 */
export const SHEET_GAP = 24;

/** 栅格宽度边界与量化步长（交给 Rust 的 `?w=`，设备像素）。 */
export const RASTER_MIN = 720;
export const RASTER_MAX = 2400;
const RASTER_STEP = 120;

/** 比例未知时的兜底（正常情况 Rust 的 `pageSizes` 一定给）。 */
export const A4_ASPECT = 210 / 297;

export type SheetSize = { width: number; height: number };

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));

/** 一行的页号列表：封面单独，之后 2-3、4-5…（单页时一行一张）。 */
export function pageRows(
  pageCount: number,
  spread: PdfResolvedSpread,
): number[][] {
  const rows: number[][] = [];
  const count = Math.max(0, Math.floor(pageCount));
  if (count === 0) return rows;
  if (spread === "single") {
    for (let page = 1; page <= count; page += 1) rows.push([page]);
    return rows;
  }
  rows.push([1]);
  for (let page = 2; page <= count; page += 2) {
    rows.push(page + 1 <= count ? [page, page + 1] : [page]);
  }
  return rows;
}

/** 含 `page` 的那一行的起始页：把这一行滚到视口顶部，就等于把这一页滚到顶部。 */
export function rowStartOf(page: number, spread: PdfResolvedSpread): number {
  if (spread === "single") return Math.max(1, page);
  if (page <= 1) return 1;
  return page % 2 === 0 ? page : page - 1;
}

/**
 * 一张纸画多大（CSS px）。`aspect` = 宽 ÷ 高。
 *
 * - 适应宽度：纸宽 = 阅读区宽（双页各占一半），高按比例。
 * - 适应页面：纸高 = 阅读区高，宽按比例；双页时两张纸＋中缝要塞得进宽度，
 *   塞不进就整体按比例缩小（手动选双页而窗口不够宽时，缩小比横向裁掉好）。
 */
export function sheetSize(
  aspect: number,
  fit: PdfFit,
  spread: PdfResolvedSpread,
  areaWidth: number,
  areaHeight: number,
): SheetSize {
  const ratio = aspect > 0 ? aspect : A4_ASPECT;
  const areaW = Math.max(1, areaWidth);
  const areaH = Math.max(1, areaHeight);
  if (fit === "width") {
    const width =
      spread === "double" ? Math.max(1, (areaW - SHEET_GAP) / 2) : areaW;
    return { width, height: width / ratio };
  }
  let height = areaH;
  let width = height * ratio;
  if (spread === "double" && 2 * width + SHEET_GAP > areaW) {
    const scale = Math.max(0.05, (areaW - SHEET_GAP) / (2 * width));
    width *= scale;
    height *= scale;
  }
  return { width, height };
}

/**
 * 「自动」张数判据——用真实页面宽高比，不用窗口比例猜。
 *
 * 判据：**两张纸按「高度贴合」并排，还塞得进阅读区宽吗**
 * （`2 × 区高 × aspect + 中缝 ≤ 区宽`）。这正是适应页面下单页纸的宽度，所以判据
 * 就是「两张高度贴合的纸放得下吗」；**横向页**会算出很宽 → 自动单页，竖屏窗口
 * （高 > 宽）也一律单页。
 *
 * 这是纯函数，两种缩放都能算；但**壳只在「适应页面」下用它**——适应宽度的语义是
 * 「纸宽贴合窗口、一次一张」（见 `App.tsx` 顶栏：适应宽度时 spread 恒为 single），
 * 想并排阅读就用「适应页面 + 双页」。
 */
export function autoSpread(
  aspect: number,
  areaWidth: number,
  areaHeight: number,
): PdfResolvedSpread {
  if (!(aspect > 0) || areaWidth <= 0 || areaHeight <= 0) return "single";
  if (areaHeight > areaWidth) return "single";
  return 2 * areaHeight * aspect + SHEET_GAP <= areaWidth ? "double" : "single";
}

/** 用户设置 → 实际张数（手动项一律照办，只有「自动」去算）。
 *  壳在适应宽度下直接给 `"single"`（见 `autoSpread` 的说明）。 */
export function resolveSpread(
  setting: PdfSpread,
  aspect: number,
  areaWidth: number,
  areaHeight: number,
): PdfResolvedSpread {
  return setting === "auto"
    ? autoSpread(aspect, areaWidth, areaHeight)
    : setting;
}

/**
 * 栅格宽度：**纸实际画多大** × DPR，量化到 [`RASTER_STEP`]、钳在
 * [`RASTER_MIN`]–[`RASTER_MAX`]。
 *
 * 按纸宽而不是容器宽算：竖版纸按高度贴合后可能只有 ~500px 宽，按 1600 的容器宽请求
 * 就是白花三倍栅格时间与字节。量化保证「窗口慢慢拉动」与「兜底尺寸修正」都不来回抖
 * ——同档就不换 URL（Rust 那边另有分桶与预取）。
 */
export function rasterWidth(cssWidth: number, dpr: number): number {
  const scale = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  const raw = Math.max(320, cssWidth) * scale;
  const quantised = Math.round(raw / RASTER_STEP) * RASTER_STEP;
  return clamp(quantised, RASTER_MIN, RASTER_MAX);
}

/**
 * 一页的图片 URL。契约归 Rust（`crates/formats-pdf/src/book.rs`）：`page/NNNN.webp?w=`，
 * 前端只拼、不解析 PDF。
 */
export function sheetUrl(
  origin: string,
  bookId: string,
  page: number,
  width: number,
): string {
  const padded = String(Math.max(1, Math.round(page))).padStart(4, "0");
  return `${origin}/book/${bookId}/page/${padded}.webp?w=${Math.max(1, Math.round(width))}`;
}
