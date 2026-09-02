/**
 * CSS multi-column pagination used by Foliate / Epub.js.
 * Column count is min(maxColumnCount, ceil(container / maxInlineSize)).
 * Extra window width becomes side margin, not wider text.
 */

export const FLOW_STYLE_ID = "iced-reader-flow";
/** Max width of one text column (Foliate `--_max-inline-size`). */
export const MAX_INLINE_SIZE = 720;
export const MAX_COLUMN_COUNT = 2;
/** Requested gap as a fraction of the container; actual gap is r/(1-r)·size. */
export const GAP_RATIO = 0.07;
/** Top/bottom inset as a fraction of view height; clamped in `blockPadding`. */
export const BLOCK_PAD_RATIO = 0.04;
export const BLOCK_PAD_MIN = 20;
export const BLOCK_PAD_MAX = 40;

export type FlowMetrics = {
  columns: 1 | 2;
  columnWidth: number;
  gap: number;
  /** One spread: the scrollport width. Turn page by this many pixels. */
  stride: number;
  viewWidth: number;
  viewHeight: number;
};

export function columnGap(size: number, ratio = GAP_RATIO): number {
  if (size <= 0) return 0;
  return (ratio / (1 - ratio)) * size;
}

export function blockPadding(height: number): number {
  if (height <= 0) return BLOCK_PAD_MIN;
  return Math.round(
    Math.max(BLOCK_PAD_MIN, Math.min(BLOCK_PAD_MAX, height * BLOCK_PAD_RATIO)),
  );
}

export function columnDivisor(
  size: number,
  maxInline = MAX_INLINE_SIZE,
  maxCols = MAX_COLUMN_COUNT,
): 1 | 2 {
  const n = Math.min(maxCols, Math.max(1, Math.ceil(size / maxInline)));
  return n >= 2 ? 2 : 1;
}

export function flowMetrics(
  viewWidth: number,
  viewHeight: number,
  portrait: boolean,
): FlowMetrics {
  const width = Math.max(1, viewWidth);
  const height = Math.max(1, viewHeight);
  const maxCols = portrait ? 1 : MAX_COLUMN_COUNT;
  const columns = columnDivisor(width, MAX_INLINE_SIZE, maxCols);
  const gap = columnGap(width);
  const columnWidth = Math.max(1, width / columns - gap);
  return {
    columns,
    columnWidth,
    gap,
    stride: width,
    viewWidth: width,
    viewHeight: height,
  };
}

export function pageCountFromContent(contentWidth: number, stride: number): number {
  if (stride <= 0) return 1;
  return Math.max(1, Math.ceil(contentWidth / stride - 1e-6));
}

export function pageIndexFromFraction(fraction: number, pages: number): number {
  if (pages <= 1) return 0;
  const t = Number.isFinite(fraction) ? fraction : 0;
  return Math.min(pages - 1, Math.max(0, Math.round(t * (pages - 1))));
}

export function fractionFromPageIndex(page: number, pages: number): number {
  if (pages <= 1) return 0;
  return Math.min(1, Math.max(0, page / (pages - 1)));
}

export function scrollLeftForPage(page: number, stride: number): number {
  return Math.max(0, page) * stride;
}

export function flowCss(metrics: FlowMetrics): string {
  const w = Math.trunc(metrics.columnWidth);
  const g = metrics.gap.toFixed(2);
  const h = metrics.viewHeight.toFixed(2);
  const padX = (metrics.gap / 2).toFixed(2);
  const padY = blockPadding(metrics.viewHeight);
  const imgH = Math.max(1, metrics.viewHeight - padY * 2).toFixed(2);
  return `
html.${FLOW_STYLE_ID} {
  box-sizing: border-box !important;
  height: ${h}px !important;
  column-width: ${w}px !important;
  column-gap: ${g}px !important;
  column-fill: auto !important;
  padding: ${padY}px ${padX}px !important;
  overflow: hidden !important;
  overflow-wrap: break-word !important;
  position: static !important;
  border: 0 !important;
  margin: 0 !important;
  max-height: none !important;
  max-width: none !important;
  min-height: 0 !important;
  min-width: 0 !important;
}
html.${FLOW_STYLE_ID} body {
  margin: 0 !important;
  max-height: none !important;
  max-width: none !important;
}
html.${FLOW_STYLE_ID} img,
html.${FLOW_STYLE_ID} svg,
html.${FLOW_STYLE_ID} video {
  max-width: 100% !important;
  max-height: ${imgH}px !important;
  object-fit: contain !important;
  break-inside: avoid !important;
  page-break-inside: avoid !important;
  box-sizing: border-box !important;
}
`;
}

export function contentWidth(doc: Document): number {
  const body = doc.body;
  const root = doc.documentElement;
  if (!body || !root) return root?.getBoundingClientRect().width ?? 0;
  const range = doc.createRange();
  range.selectNodeContents(body);
  const content = range.getBoundingClientRect();
  const rootRect = root.getBoundingClientRect();
  const start = content.left - rootRect.left;
  return Math.max(0, start + content.width);
}
