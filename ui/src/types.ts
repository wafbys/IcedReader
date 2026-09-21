export type Metadata = {
  title: string;
  authors: string[];
  language: string | null;
  publisher: string | null;
  identifiers: string[];
  description: string | null;
  coverHref: string | null;
};

export type TocNode = {
  label: string;
  href: string | null;
  children: TocNode[];
};

export type SpineItem = {
  id: string;
  href: string;
  mediaType: string;
  title?: string | null;
};

export type Locator = {
  href: string;
  fraction: number;
  cfi: string | null;
};

/**
 * One user highlight. Anchored inside one chapter by the global text-node
 * sequence + in-node offset (stable because chapter HTML is deterministic),
 * plus an excerpt used for validation/display. `href` matches the spine unit.
 * `color` is chosen at stroke time and never edited afterwards (换色 = 删除
 * 重划): yellow = 重点 (default), green = 摘抄. `pos` is the whole-book
 * position 0–1 from per-chapter raw visible-text char weights (same char
 * regime as the front-end text nodes), written into notes.md and used by
 * 按位置跳转.
 */
export type HighlightRecord = {
  id: string;
  href: string;
  startText: number;
  startOffset: number;
  endText: number;
  endOffset: number;
  text: string;
  color: string;
  pos: number;
  createdAt: number;
};

export type LibraryEntry = {
  path: string;
  fileName: string;
  title: string;
  authors: string[];
  progressKey: string;
  chapterIndex: number | null;
  chapterCount: number | null;
  chapterTitle: string | null;
  fraction: number | null;
  updatedAt: number | null;
  hasCover: boolean;
  coverRev: string;
  /** 书籍文件字节数（0 = 取不到）。书架 tooltip / 编辑元数据面板显示。 */
  sizeBytes: number;
  openError: string | null;
  /** 优/良/中 from the cached first-import signals (null when unknown). */
  quality: string | null;
  /** 支持该评级的正面事实（正文字数、无乱码、作者、标识符…）。 */
  qualityPlus: string[];
  /** 把它从更高评级拉下来的扣分项（乱码、缺作者、无标识符…），无则为空。 */
  qualityMinus: string[];
  /** Other library books judged the same book (hint only). */
  duplicates: string[];
};

/** 编辑元数据面板（get_book_meta）的载荷。 */
export type BookMetaView = {
  fileName: string;
  /** 只读：首次导入时程序见到的书名（before any user edit）。 */
  originalTitle: string;
  /** 主书名 — 预填伴生 md 值或清洗后的当前书名（必填）。 */
  title: string;
  subtitle: string;
  volume: string;
  /** 作者 — 预填伴生 md 值或原书 dc:creator（多名用、连接）。 */
  author: string;
  /** 译者 — 拼入标题时自动补「译者 」标签。 */
  translator: string;
  /** 出版年份。 */
  year: string;
  publisher: string;
  isbn: string;
  /** 手改框初值：md 里用户确认过的 displayTitle；空 = 未确认，由字段拼接接管。 */
  confirmedTitle: string;
  /** 当前裁决结果（书架/阅读正在显示的名字，永远非空）。 */
  displayTitle: string;
  /** 由当前字段拼出的候选（“自动填充”把此值写入手改框）。 */
  suggestedTitle: string;
};

/** 保存到 set_book_meta 的字段（displayTitle 空 = 派生模式）。 */
export type BookMetaFields = {
  title: string;
  subtitle: string;
  volume: string;
  author: string;
  translator: string;
  year: string;
  publisher: string;
  isbn: string;
  displayTitle: string;
};

export type OpenedBook = {
  id: string;
  /** `"epub"` | `"pdf"` — the reading shell branches on this (AGENTS「改 UI 时」). */
  format: string;
  path: string;
  progressKey: string;
  progress: Locator | null;
  metadata: Metadata;
  toc: TocNode[];
  spine: SpineItem[];
  /** Per-chapter raw visible-text char counts (spine order). Whole-book
   *  position weights for notes.md 全书% and 按位置跳转. */
  chapterChars: number[];
  /** 打开时的格式特有提示（中文，可直接显示）。EPUB 恒为空数组；PDF 正文若用了
   *  未嵌入的非标准字体，这里有一条「可能缺字」提示。 */
  warnings: string[];
  /** 每页 `[宽, 高]`（PDF 单位，只用比例）。PDF 连续纸带靠它从第一帧就铺出正确
   *  占位（滚动条长度、跳页都准），不必等图片加载。EPUB 恒为空数组。 */
  pageSizes: [number, number][];
};

export type FontSlotId = "serif" | "sans" | "mono" | "cjk";

export type FontFile = {
  file: string;
  originalName: string;
};

export type PublisherFontDecl = {
  selector: string;
  value: string;
  source: string;
};

export type UsedFontSource = "specified" | "fallback" | "generic";

export type UsedFontEntry = {
  family: string;
  glyphCount: number;
  source: UsedFontSource;
  sample: string;
  via?: string;
};

export type UsedFontReport = {
  fonts: UsedFontEntry[];
  missingSpecified: string[];
  error?: string;
};

export type PublisherFontReport = {
  declarations: PublisherFontDecl[];
  faces: string[];
  unloadableFaces?: string[];
  truncated: boolean;
};

export type ChapterPayload = {
  html: string;
  publisherFonts: PublisherFontReport;
};

export type FontSettings = {
  useOriginalFonts: boolean;
  fonts: {
    serif: FontFile | null;
    sans: FontFile | null;
    mono: FontFile | null;
    cjk: FontFile | null;
  };
  missingSlots: FontSlotId[];
  customFontsActive: boolean;
  fontScale: number;
};

export const FONT_SLOTS: { id: FontSlotId; label: string }[] = [
  { id: "serif", label: "衬线（serif）" },
  { id: "sans", label: "无衬线（sans）" },
  { id: "mono", label: "等宽（mono）" },
  { id: "cjk", label: "中文 / CJK" },
];

export function slotLabel(id: FontSlotId): string {
  return FONT_SLOTS.find((s) => s.id === id)?.label ?? id;
}

export function normHref(href: string, keepFragment = false): string {
  const hash = href.indexOf("#");
  const filePart = (hash >= 0 ? href.slice(0, hash) : href)
    .split("?")[0]
    .replace(/^\/+/, "")
    .toLowerCase();
  if (!keepFragment || hash < 0) return filePart;
  const fragment = href.slice(hash + 1).split("?")[0];
  return fragment ? `${filePart}#${fragment}` : filePart;
}

export function chapterIndex(spine: SpineItem[], href: string | undefined): number {
  if (!href) return -1;
  const exact = normHref(href, true);
  const exactIdx = spine.findIndex((item) => normHref(item.href, true) === exact);
  if (exactIdx >= 0) return exactIdx;
  const file = normHref(href);
  return spine.findIndex((item) => normHref(item.href) === file);
}

/** 同书对照（compare_books）的载荷。规则见 docs/ideas/book-compare.md。 */

/** 这几本文件之间是什么关系。 */
export type CompareRelationKind =
  | "sameTypesetting"
  | "sameEdition"
  | "contained"
  | "partition"
  | "unrelated";

export type CompareAxisGroup =
  | "content"
  | "apparatus"
  | "packaging"
  | "provenance";

/** `presented` = 只摆数字不判优劣；`incomparable` = 数据缺失，比不了。 */
export type CompareVerdict =
  | "tie"
  | "winner"
  | "shared"
  | "incomparable"
  | "presented";

export type CompareCellMark = "best" | "worst" | "tie" | "unknown";

export type CompareCell = {
  display: string;
  num: number | null;
  mark: CompareCellMark;
};

export type CompareAxis = {
  key: string;
  label: string;
  group: CompareAxisGroup;
  /** 这项是怎么量出来的，显示在行标题的悬停里。 */
  note: string | null;
  cells: CompareCell[];
  verdict: CompareVerdict;
  /** 该轴胜出的列下标，只有 winner / shared 时非空。 */
  winners: number[];
};

export type CompareColumn = {
  fileName: string;
  title: string;
  /** 这本书自己的 优/良/中，与本次对照无关。 */
  quality: string | null;
  sizeBytes: number;
};

/** 两本同回目时的逐章差额，用来画条带。 */
export type CompareChapterDiff = {
  labels: string[];
  /** chars[b] - chars[a]，正数表示第二本这一章更长。 */
  deltas: number[];
  identical: boolean[];
};

export type BookComparison = {
  kind: CompareRelationKind;
  kindNote: string;
  columns: CompareColumn[];
  axes: CompareAxis[];
  chapterDiff: CompareChapterDiff | null;
  /** 倾向保留的列下标；只有唯一赢家时非空。 */
  lean: number | null;
  /** 它凭什么赢，一行一个轴。 */
  leanReason: string[];
  conclusion: string[];
};
