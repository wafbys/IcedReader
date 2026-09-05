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
