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
};

export type Locator = {
  href: string;
  fraction: number;
  cfi: string | null;
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
};

export type UsedFontReport = {
  fonts: UsedFontEntry[];
  missingSpecified: string[];
  error?: string;
};

export type PublisherFontReport = {
  declarations: PublisherFontDecl[];
  faces: string[];
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

export function normHref(href: string): string {
  return href.split("#")[0].split("?")[0].replace(/^\/+/, "").toLowerCase();
}

export function chapterIndex(spine: SpineItem[], href: string | undefined): number {
  if (!href) return -1;
  const target = normHref(href);
  return spine.findIndex((item) => normHref(item.href) === target);
}
