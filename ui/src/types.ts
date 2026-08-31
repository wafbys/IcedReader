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

export function normHref(href: string): string {
  return href.split("#")[0].split("?")[0].replace(/^\/+/, "").toLowerCase();
}

export function chapterIndex(spine: SpineItem[], href: string | undefined): number {
  if (!href) return -1;
  const target = normHref(href);
  return spine.findIndex((item) => normHref(item.href) === target);
}
