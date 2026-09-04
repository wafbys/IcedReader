import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import {
  FLOW_STYLE_ID,
  contentWidth,
  flowCss,
  flowMetrics,
  fractionFromPageIndex,
  pageCountFromContent,
  pageIndexFromFraction,
  scrollLeftForPage,
  type FlowMetrics,
} from "./flowLayout";
import {
  anchorFromRange,
  anchorOverlapInfo,
  charAtPoint,
  highlightSupported,
  paintHighlights,
  rangeForRecord,
  spanAtChar,
  type AppliedSpan,
  type HighlightAnchor,
} from "./highlights";
import { collectUsedFonts, type UsedFontReport } from "./usedFonts";
import { ensureCoverFit } from "./coverFit";
import { ensureWordNoteStyle } from "./wordNotes";
import type { HighlightRecord } from "./types";
import { normHref } from "./types";

export type PageInfo = {
  page: number;
  pages: number;
  columns: 1 | 2;
};

export type ChapterFrameHandle = {
  goPage: (delta: number) => "ok" | "before" | "after";
  goToPage: (page: number) => void;
};

type Props = {
  html: string;
  restoreFraction: number;
  authorFamilies: string[];
  documentLang?: string | null;
  onProgress: (fraction: number) => void;
  onUsedFonts: (report: UsedFontReport) => void;
  onPageInfo: (info: PageInfo) => void;
  onNeedChapter: (delta: -1 | 1) => void;
  /** A book-internal link was clicked: navigate to another chapter file. */
  onFollowBookHref?: (href: string) => void;
  /** Highlights belonging to the currently displayed chapter. */
  highlights: HighlightRecord[];
  /** Spine href of the chapter this frame is showing (for paint sync). */
  chapterHref: string;
  onCreateHighlight: (href: string, anchor: HighlightAnchor) => Promise<void>;
  onDeleteHighlight: (id: string) => Promise<void>;
  /** A highlight to scroll into view once its chapter is laid out. */
  pendingHighlight: HighlightRecord | null;
  /** Called once the pending highlight has been located (clears it upstream). */
  onHighlightLocated: () => void;
  fontScale?: number;
};

/** Floating mini toolbar; x/y are parent-viewport CSS pixels. */
type ToolbarState =
  | {
      kind: "create";
      x: number;
      y: number;
      /** Chapter href captured together with the anchor (doc this frame shows). */
      href: string;
      anchor: HighlightAnchor;
      overlaps: boolean;
      /** Single existing highlight fully containing the selection (delete it). */
      containedId?: string | null;
    }
  | { kind: "delete"; x: number; y: number; id: string }
  | null;

function usableLang(value: string | null | undefined): string | null {
  if (!value) return null;
  const t = value.trim();
  if (!t || /^und$/i.test(t) || /^zxx$/i.test(t)) return null;
  if (!/^[\w-]+$/.test(t)) return null;
  return t;
}

/** HTML parsers ignore xml:lang; Blink uses `lang` for generic serif/sans CJK. */
export function ensureHtmlLang(html: string, bookLang?: string | null): string {
  if (/\slang\s*=/i.test(html)) return html;
  const xml = html.match(/\sxml:lang\s*=\s*["']([^"']+)["']/i);
  const lang = usableLang(xml?.[1]) ?? usableLang(bookLang);
  if (!lang) return html;
  if (/<html\b/i.test(html)) {
    return html.replace(/<html\b/i, `<html lang="${lang}"`);
  }
  return html;
}

function paperBackground(doc: Document): string {
  const win = doc.defaultView;
  if (!win) return "";
  const body = win.getComputedStyle(doc.body);
  const transparent =
    body.backgroundColor === "rgba(0, 0, 0, 0)" && body.backgroundImage === "none";
  return transparent
    ? win.getComputedStyle(doc.documentElement).background
    : body.background;
}

type LayoutState = {
  doc: Document | null;
  metrics: FlowMetrics | null;
  page: number;
  pages: number;
};

const ChapterFrame = forwardRef<ChapterFrameHandle, Props>(function ChapterFrame(
  {
    html,
    restoreFraction,
    authorFamilies,
    documentLang,
    onProgress,
    onUsedFonts,
    onPageInfo,
    onNeedChapter,
    onFollowBookHref,
    highlights,
    chapterHref,
    onCreateHighlight,
    onDeleteHighlight,
    pendingHighlight,
    onHighlightLocated,
    fontScale = 100,
  },
  ref,
) {
  const hostRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const gen = useRef(0);
  const fractionRef = useRef(restoreFraction);
  const htmlRef = useRef(html);
  if (htmlRef.current !== html) {
    htmlRef.current = html;
    fractionRef.current = restoreFraction;
  }
  const layout = useRef<LayoutState>({
    doc: null,
    metrics: null,
    page: 0,
    pages: 1,
  });
  const onProgressRef = useRef(onProgress);
  onProgressRef.current = onProgress;
  const onUsedFontsRef = useRef(onUsedFonts);
  onUsedFontsRef.current = onUsedFonts;
  const onPageInfoRef = useRef(onPageInfo);
  onPageInfoRef.current = onPageInfo;
  const onNeedChapterRef = useRef(onNeedChapter);
  onNeedChapterRef.current = onNeedChapter;
  const onFollowBookHrefRef = useRef(onFollowBookHref);
  onFollowBookHrefRef.current = onFollowBookHref;
  const authorRef = useRef(authorFamilies);
  authorRef.current = authorFamilies;
  const fontScaleRef = useRef(fontScale);
  fontScaleRef.current = fontScale;
  const highlightsRef = useRef(highlights);
  highlightsRef.current = highlights;
  const onCreateHighlightRef = useRef(onCreateHighlight);
  onCreateHighlightRef.current = onCreateHighlight;
  const onDeleteHighlightRef = useRef(onDeleteHighlight);
  onDeleteHighlightRef.current = onDeleteHighlight;
  const onLocatedRef = useRef(onHighlightLocated);
  onLocatedRef.current = onHighlightLocated;
  /** Locate request; `done` marks the first placement so a font reflow can
   *  then make one precise final jump before the request is cleared. */
  const pendingRef = useRef<{ rec: HighlightRecord; done: boolean } | null>(
    null,
  );

  const wheelLock = useRef(false);
  const appliedRef = useRef<AppliedSpan[]>([]);
  const paintedRef = useRef<{ doc: Document | null; hl: readonly HighlightRecord[] | null }>({
    doc: null,
    hl: null,
  });
  /** Doc currently shown in the iframe and the chapter it was built for. */
  const docForRef = useRef<{ doc: Document | null; href: string }>({
    doc: null,
    href: "",
  });
  /** Set when a mouseup lands on an existing highlight; the following click
   *  inside the chapter doc is then swallowed (no link navigation) so the
   *  delete toolbar stays the sole result. */
  const swallowDocClick = useRef(false);
  /** Word-note hover tooltip: viewport-fixed bubble + the marker currently
   *  hovered (mousemove keeps it anchored; leaving / page moves hide it). */
  const noteTipRef = useRef<HTMLDivElement>(null);
  const noteTipTargetRef = useRef<Element | null>(null);
  const [toolbar, setToolbar] = useState<ToolbarState>(null);
  const [toolbarBusy, setToolbarBusy] = useState(false);

  /** Repaint highlights on the current iframe document (doc must be ready). */
  const paintDoc = () => {
    const f = docForRef.current;
    const doc = iframeRef.current?.contentDocument ?? null;
    if (!f.doc || f.doc !== doc || !doc?.body) return;
    if (paintedRef.current.doc === doc && paintedRef.current.hl === highlightsRef.current) {
      return;
    }
    const applied = paintHighlights(doc, highlightsRef.current);
    appliedRef.current = applied ?? [];
    paintedRef.current = { doc, hl: highlightsRef.current };
  };

  /** Hide the word-note hover tooltip (no-op when already hidden). */
  const hideNoteTip = () => {
    noteTipTargetRef.current = null;
    noteTipRef.current?.classList.remove("visible");
  };

  /**
   * Reveal the trailing back link of any word-note item that a column break
   * really split (note items refuse breaks, but one taller than a column is
   * still cut). Reads only cached geometry after the forced reflow inside
   * `relayout`, and runs again on every relayout, so the class stays in sync
   * with window size / font scale / font-loading reflows. Showing the tail
   * can itself shift a borderline item by a line, so a second pass converges
   * when the first pass changed anything.
   */
  const syncNoteCrossState = (doc: Document) => {
    const apply = () => {
      const items = doc.querySelectorAll("p.wr-note-item");
      let changed = false;
      for (const item of items) {
        const crossed = item.getClientRects().length > 1;
        if (item.classList.contains("wr-note-cross") !== crossed) {
          item.classList.toggle("wr-note-cross", crossed);
          changed = true;
        }
      }
      return changed;
    };
    if (apply()) {
      void doc.documentElement.offsetHeight;
      apply();
    }
  };

  /**
   * Place the word-note tooltip at a parent-viewport point. When the text is
   * given and differs, the bubble is re-filled first; a hover that only moves
   * repositions without touching the text. The bubble flips to the other side
   * of the cursor when it would run off the window edge.
   */
  const placeNoteTip = (parentX: number, parentY: number, text?: string) => {
    const el = noteTipRef.current;
    if (!el) return;
    if (text !== undefined && el.textContent !== text) {
      el.textContent = text;
    }
    const w = el.offsetWidth || 320;
    const h = el.offsetHeight || 60;
    let left = parentX + 14;
    let top = parentY + 16;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    if (left + w + 8 > vw) left = parentX - w - 12;
    if (left < 4) left = 4;
    if (top + h + 8 > vh) top = parentY - h - 12;
    if (top < 4) top = 4;
    el.style.left = `${Math.round(left)}px`;
    el.style.top = `${Math.round(top)}px`;
    el.classList.add("visible");
  };

  const applyPage = (page: number, announce: boolean) => {
    const st = layout.current;
    const box = containerRef.current;
    if (!st.metrics || !box) return;
    // The content under the cursor moves, so a stale bubble must not linger.
    hideNoteTip();
    const pages = Math.max(1, st.pages);
    const next = Math.min(pages - 1, Math.max(0, page));
    st.page = next;
    box.scrollLeft = scrollLeftForPage(next, st.metrics.stride);
    const fraction = fractionFromPageIndex(next, pages);
    if (announce) {
      fractionRef.current = fraction;
      onProgressRef.current(fraction);
      setToolbar(null);
    }
    onPageInfoRef.current({
      page: next,
      pages,
      columns: st.metrics.columns,
    });
  };

  /** Scroll the flow so a highlight's span sits on the current page. */
  const locateRecord = (doc: Document, rec: HighlightRecord): boolean => {
    const st = layout.current;
    if (!st.metrics) return false;
    const range = rangeForRecord(doc, rec);
    if (!range) return false;
    const rect = range.getBoundingClientRect();
    const rootRect = doc.documentElement.getBoundingClientRect();
    const x = rect.left - rootRect.left;
    const page = Math.floor(x / st.metrics.stride);
    applyPage(page, true);
    return true;
  };

  /**
   * Scroll the flow so an element's first fragment sits on the current page.
   * In the multi-column flow a long target (e.g. a note block that crosses a
   * page break) is split into several fragments; use the first one so the
   * jump lands on the page where the target actually starts. `getClientRects`
   * is empty for zero-size anchors (an empty `<a id=…>`), fall back to the
   * bounding box, which still carries the inline position.
   */
  const jumpToFragment = (doc: Document, frag: string) => {
    const st = layout.current;
    if (!frag || !st.metrics) return;
    const el = doc.getElementById(frag);
    if (!el) return;
    const rootRect = doc.documentElement.getBoundingClientRect();
    const rects = el.getClientRects();
    const r = rects.length > 0 ? rects[0] : el.getBoundingClientRect();
    applyPage(Math.floor((r.left - rootRect.left) / st.metrics.stride), true);
  };

  /**
   * Links inside a chapter are rewritten to the app's own origin
   * (`http://icedreader.localhost/book/{id}/...`) so the book's images and CSS
   * load. A plain click would navigate the iframe away from its `srcDoc` to
   * that cross-origin resource, breaking pagination and leaving the reader
   * unable to page. Intercept: same-file anchors scroll to the element's page,
   * other files hand the navigation to the reader (chapter switch).
   *
   * Fragment-only hrefs (`#…`) never leave the `about:srcdoc` base and are
   * handled as same-file anchors too — the Rust note expansion emits absolute
   * URLs for word notes, but some in-book anchor links stay `#…`.
   */
  const followBookLink = (doc: Document, rawHref: string) => {
    const st = layout.current;
    if (!st.metrics) return;
    if (rawHref.startsWith("#")) {
      jumpToFragment(doc, rawHref.slice(1));
      return;
    }
    let url: URL;
    try {
      url = new URL(rawHref, doc.location.href);
    } catch {
      return;
    }
    if (url.protocol !== "http:" && url.protocol !== "https:") return;
    // Rewritten book links are absolute under `http://icedreader.localhost/book/{id}/`;
    // strip that prefix to recover the spine-style path (e.g. text/part0008.html).
    let bookPath = url.pathname;
    const prefix = /^\/book\/[^/]+\//.exec(bookPath);
    if (prefix) bookPath = bookPath.slice(prefix[0].length);
    const path = normHref(bookPath, false);
    if (!path) return;
    const currentPath = normHref(chapterHref, false);
    if (path === currentPath) {
      jumpToFragment(doc, url.hash.slice(1));
      return;
    }
    onFollowBookHrefRef.current?.(`${bookPath}${url.hash || ""}`);
  };

  /**
   * Place the pending highlight onto the visible page. The first placement
   * happens as soon as the chapter is laid out; when fonts are still loading
   * (which can reflow the columns), a second precise jump runs once
   * `document.fonts.ready` resolves.
   */
  const tryLocate = () => {
    const pending = pendingRef.current;
    if (!pending) return;
    if (normHref(pending.rec.href, true) !== normHref(docForRef.current.href, true)) {
      return;
    }
    const doc = docForRef.current.doc;
    if (!doc?.body || !layout.current.metrics) return;
    if (pending.done) return;
    if (!locateRecord(doc, pending.rec)) return;
    pending.done = true;
    const fonts = (doc as Document & { fonts?: FontFaceSet }).fonts;
    if (fonts && fonts.status !== "loaded") {
      void fonts.ready.then(() => {
        if (!pendingRef.current || pendingRef.current.rec.id !== pending.rec.id) {
          return;
        }
        const now = docForRef.current;
        if (
          now.doc?.body &&
          layout.current.metrics &&
          normHref(pending.rec.href, true) === normHref(now.href, true)
        ) {
          locateRecord(now.doc, pending.rec);
        }
        pendingRef.current = null;
        onLocatedRef.current();
      });
      return;
    }
    pendingRef.current = null;
    onLocatedRef.current();
  };

  const relayout = (keepFraction: boolean) => {
    const host = hostRef.current;
    const box = containerRef.current;
    const iframe = iframeRef.current;
    const doc = iframe?.contentDocument;
    if (!host || !box || !iframe || !doc?.documentElement || !doc.body) return;
    const portrait = host.clientHeight > host.clientWidth;
    host.classList.toggle("portrait", portrait);
    const width = box.clientWidth;
    const height = box.clientHeight;
    if (width < 8 || height < 8) return;

    const metrics = flowMetrics(width, height, portrait);
    let style = doc.getElementById(FLOW_STYLE_ID) as HTMLStyleElement | null;
    if (!style) {
      style = doc.createElement("style");
      style.id = FLOW_STYLE_ID;
      doc.head.appendChild(style);
    }
    doc.documentElement.classList.add(FLOW_STYLE_ID);
    style.textContent = flowCss(metrics, fontScaleRef.current);
    doc.documentElement.style.setProperty("width", `${metrics.stride}px`, "important");
    void doc.documentElement.offsetHeight;
    ensureWordNoteStyle(doc);
    ensureCoverFit(doc);
    syncNoteCrossState(doc);

    const pages = pageCountFromContent(contentWidth(doc), metrics.stride);
    iframe.style.width = `${pages * metrics.stride}px`;
    iframe.style.height = "100%";

    const bg = paperBackground(doc);
    if (bg) host.style.background = bg;

    layout.current.doc = doc;
    layout.current.metrics = metrics;
    layout.current.pages = pages;
    const page = keepFraction
      ? pageIndexFromFraction(fractionRef.current, pages)
      : 0;
    applyPage(page, false);
    tryLocate();
  };

  const goPage = (delta: number): "ok" | "before" | "after" => {
    const st = layout.current;
    if (!st.metrics) return "ok";
    const next = st.page + delta;
    if (next < 0) return "before";
    if (next >= st.pages) return "after";
    applyPage(next, true);
    return "ok";
  };

  useImperativeHandle(
    ref,
    () => ({
      goPage,
      goToPage: (page: number) => applyPage(page, true),
    }),
    [],
  );

  useEffect(() => {
    fractionRef.current = restoreFraction;
  }, [restoreFraction, html]);

  // Chapter content changed: the old doc's toolbar is stale and its anchor
  // would be committed against the wrong chapter, so drop it now.
  useEffect(() => {
    setToolbar(null);
    setToolbarBusy(false);
  }, [html]);

  useEffect(() => {
    relayout(true);
  }, [fontScale]);

  useEffect(() => {
    const f = docForRef.current;
    if (f.href === chapterHref) paintDoc();
  }, [highlights, chapterHref]);

  useEffect(() => {
    if (pendingHighlight) {
      const cur = pendingRef.current;
      if (!cur || cur.rec.id !== pendingHighlight.id) {
        pendingRef.current = { rec: pendingHighlight, done: false };
      }
      // Same chapter already rendered: locate immediately; a cross-chapter
      // jump waits for the next iframe load -> relayout -> tryLocate.
      tryLocate();
    } else {
      pendingRef.current = null;
    }
  }, [pendingHighlight]);

  const turn = (dir: -1 | 1) => {
    setToolbar(null);
    const result = goPage(dir);
    if (result === "before") onNeedChapterRef.current(-1);
    if (result === "after") onNeedChapterRef.current(1);
  };

  const hideToolbar = () => setToolbar(null);

  /** Show a highlight; called with parent-viewport coordinates. */
  const docMouseUp = (e: MouseEvent, doc: Document, iframe: HTMLIFrameElement) => {
    const iframeRect = iframe.getBoundingClientRect();
    const parentX = iframeRect.left + e.clientX;
    const parentY = iframeRect.top + e.clientY;
    const sel = doc.getSelection();
    const text = sel ? sel.toString() : "";

    if (sel && !sel.isCollapsed && text.trim()) {
      if (!highlightSupported(doc)) return;
      const range = sel.getRangeAt(0);
      const anchor = anchorFromRange(doc, range);
      if (!anchor) return;
      const info = anchorOverlapInfo(doc, highlightsRef.current, anchor);
      const rect = range.getBoundingClientRect();
      const x = iframeRect.left + rect.left + rect.width / 2;
      const y = iframeRect.top + rect.top;
      setToolbar({
        kind: "create",
        x,
        y,
        href: chapterHref,
        anchor,
        overlaps: info.overlapIds.length > 0,
        containedId: info.containedId,
      });
      return;
    }

    // No text selection: clicking inside an existing highlight offers delete.
    const char = charAtPoint(doc, e.clientX, e.clientY);
    const span = char !== null ? spanAtChar(appliedRef.current, char) : null;
    if (span) {
      swallowDocClick.current = true;
      setToolbar({ kind: "delete", x: parentX, y: parentY, id: span.id });
    } else {
      setToolbar(null);
    }
  };

  const doCreate = async (href: string, anchor: HighlightAnchor) => {
    if (toolbarBusy) return;
    setToolbarBusy(true);
    try {
      await onCreateHighlightRef.current(href, anchor);
      setToolbar(null);
      const doc = iframeRef.current?.contentDocument;
      doc?.getSelection()?.removeAllRanges();
    } finally {
      setToolbarBusy(false);
    }
  };

  const doDelete = async (id: string) => {
    if (toolbarBusy) return;
    setToolbarBusy(true);
    try {
      await onDeleteHighlightRef.current(id);
      setToolbar(null);
      const doc = iframeRef.current?.contentDocument;
      doc?.getSelection()?.removeAllRanges();
    } finally {
      setToolbarBusy(false);
    }
  };

  return (
    <div className="flow-host" ref={hostRef}>
      <div
        className="flow-container"
        ref={containerRef}
        onClick={(e) => {
          const iframe = iframeRef.current;
          const doc = iframe?.contentDocument;
          if (doc?.getSelection()?.toString()) return;
          // A click on an existing highlight must not page-turn; the mouseup
          // handler already opened the delete toolbar for it.
          if (iframe && doc) {
            const iframeRect = iframe.getBoundingClientRect();
            const docX = e.clientX - iframeRect.left;
            const docY = e.clientY - iframeRect.top;
            const char = charAtPoint(doc, docX, docY);
            if (char !== null && spanAtChar(appliedRef.current, char)) {
              return;
            }
          }
          const box = containerRef.current;
          if (!box) return;
          const ratio = (e.clientX - box.getBoundingClientRect().left) / (box.clientWidth || 1);
          if (ratio > 0.28 && ratio < 0.72) return;
          turn(ratio < 0.28 ? -1 : 1);
        }}
        onWheel={(e) => {
          if (Math.abs(e.deltaY) < 4 && Math.abs(e.deltaX) < 4) return;
          e.preventDefault();
          if (wheelLock.current) return;
          wheelLock.current = true;
          window.setTimeout(() => {
            wheelLock.current = false;
          }, 280);
          turn(e.deltaY > 0 || e.deltaX > 0 ? 1 : -1);
        }}
      >
        <iframe
          ref={iframeRef}
          id="iced-chapter"
          className="chapter"
          title="chapter"
          scrolling="no"
          srcDoc={ensureHtmlLang(html, documentLang)}
          sandbox="allow-same-origin allow-popups-to-escape-sandbox"
          onLoad={() => {
            const iframe = iframeRef.current;
            const box = containerRef.current;
            const doc = iframe?.contentDocument;
            if (!iframe || !box || !doc) return;
            const token = ++gen.current;
            const live = () => token === gen.current;
            docForRef.current = { doc, href: chapterHref };

            const paint = (keep: boolean) => {
              if (!live()) return;
              relayout(keep);
              paintDoc();
            };

            paint(true);
            requestAnimationFrame(() => {
              paint(true);
              const ready = doc.fonts?.ready ?? Promise.resolve();
              void ready.then(() => {
                if (!live()) return;
                paint(true);
                onUsedFontsRef.current(collectUsedFonts(doc, authorRef.current));
                window.setTimeout(() => {
                  if (!live()) return;
                  paint(true);
                  onUsedFontsRef.current(collectUsedFonts(doc, authorRef.current));
                }, 450);
              });
            });

            const onDocWheel = (e: WheelEvent) => {
              if (Math.abs(e.deltaY) < 4 && Math.abs(e.deltaX) < 4) return;
              e.preventDefault();
              if (wheelLock.current) return;
              wheelLock.current = true;
              window.setTimeout(() => {
                wheelLock.current = false;
              }, 280);
              turn(e.deltaY > 0 || e.deltaX > 0 ? 1 : -1);
            };
            doc.addEventListener("wheel", onDocWheel, { passive: false });

            const onDocMouseUp = (e: MouseEvent) => {
              if (!live()) return;
              docMouseUp(e, doc, iframe);
            };
            doc.addEventListener("mouseup", onDocMouseUp);

            const onDocClick = (e: MouseEvent) => {
              if (!live()) return;
              if (swallowDocClick.current) {
                swallowDocClick.current = false;
                e.preventDefault();
                e.stopPropagation();
                return;
              }
              const target = e.target as Element | null;
              const anchor = target?.closest?.("a[href]") as HTMLAnchorElement | null;
              if (!anchor) return;
              // Never let a book link navigate the iframe away from srcDoc;
              // route it inside the reader instead.
              e.preventDefault();
              e.stopPropagation();
              followBookLink(doc, anchor.getAttribute("href") ?? anchor.href);
            };
            doc.addEventListener("click", onDocClick);

            // Word-note hover tooltip: a dark bubble (black bg, light text) in
            // the parent-page DOM. The marker carries its full note text in
            // `data-note` and has no `title`, so this is the only tooltip that
            // ever appears.
            const onDocMouseOver = (e: MouseEvent) => {
              if (!live()) return;
              const marker = (e.target as Element | null)?.closest?.(
                "a.wr-note",
              ) as Element | null;
              if (!marker) {
                if (noteTipTargetRef.current) hideNoteTip();
                return;
              }
              const text = marker.getAttribute("data-note") ?? "";
              if (!text) {
                hideNoteTip();
                return;
              }
              noteTipTargetRef.current = marker;
              const iframeRect = iframe.getBoundingClientRect();
              placeNoteTip(iframeRect.left + e.clientX, iframeRect.top + e.clientY, text);
            };
            const onDocMouseMove = (e: MouseEvent) => {
              if (!noteTipTargetRef.current) return;
              const marker = (e.target as Element | null)?.closest?.(
                "a.wr-note",
              ) as Element | null;
              if (!marker) {
                hideNoteTip();
                return;
              }
              const iframeRect = iframe.getBoundingClientRect();
              placeNoteTip(iframeRect.left + e.clientX, iframeRect.top + e.clientY);
            };
            doc.addEventListener("mouseover", onDocMouseOver);
            doc.addEventListener("mousemove", onDocMouseMove);

            const ro = new ResizeObserver(() => {
              if (!live()) return;
              paint(true);
            });
            ro.observe(box);
            if (hostRef.current) ro.observe(hostRef.current);

            const cleanup = () => {
              ro.disconnect();
              doc.removeEventListener("wheel", onDocWheel);
              doc.removeEventListener("mouseup", onDocMouseUp);
              doc.removeEventListener("click", onDocClick);
              doc.removeEventListener("mouseover", onDocMouseOver);
              doc.removeEventListener("mousemove", onDocMouseMove);
              hideNoteTip();
              appliedRef.current = [];
            };
            iframe.addEventListener("load", cleanup, { once: true });
          }}
        />
      </div>

      {/* Word-note hover bubble (parent-page DOM, viewport-fixed, dark). */}
      <div className="note-tip" ref={noteTipRef} role="tooltip" />

      {toolbar && (
        <div
          className={`hl-pop ${toolbar.y < 90 ? "below" : "above"}`}
          style={{ left: toolbar.x, top: toolbar.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {toolbar.kind === "create" ? (
            <>
              {toolbar.overlaps && !toolbar.containedId ? (
                <span className="hl-pop-hint">与已有划线重叠</span>
              ) : (
                <button
                  type="button"
                  className={`btn ghost small ${toolbar.overlaps ? "danger" : "paint"}`}
                  disabled={toolbarBusy}
                  title={
                    toolbar.overlaps
                      ? "已划线的这处文字，点击删除此划线"
                      : undefined
                  }
                  onClick={() => {
                    if (toolbar.overlaps && toolbar.containedId) {
                      void doDelete(toolbar.containedId);
                    } else {
                      void doCreate(toolbar.href, toolbar.anchor);
                    }
                  }}
                >
                  {toolbar.overlaps ? "删除此划线" : "划线"}
                </button>
              )}
              <button
                type="button"
                className="btn ghost small icon"
                aria-label="关闭"
                title="关闭"
                onClick={hideToolbar}
              >
                ✕
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                className="btn ghost small danger"
                disabled={toolbarBusy}
                onClick={() => void doDelete(toolbar.id)}
              >
                删除划线
              </button>
              <button
                type="button"
                className="btn ghost small icon"
                aria-label="关闭"
                title="关闭"
                onClick={hideToolbar}
              >
                ✕
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
});

export default ChapterFrame;
