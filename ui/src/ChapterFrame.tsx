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
  spanAtChar,
  type AppliedSpan,
  type HighlightAnchor,
} from "./highlights";
import { collectUsedFonts, type UsedFontReport } from "./usedFonts";
import type { HighlightRecord } from "./types";

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
  /** Highlights belonging to the currently displayed chapter. */
  highlights: HighlightRecord[];
  /** Spine href of the chapter this frame is showing (for paint sync). */
  chapterHref: string;
  onCreateHighlight: (href: string, anchor: HighlightAnchor) => Promise<void>;
  onDeleteHighlight: (id: string) => Promise<void>;
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
    highlights,
    chapterHref,
    onCreateHighlight,
    onDeleteHighlight,
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

  const applyPage = (page: number, announce: boolean) => {
    const st = layout.current;
    const box = containerRef.current;
    if (!st.metrics || !box) return;
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
              }
            };
            doc.addEventListener("click", onDocClick);

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
              appliedRef.current = [];
            };
            iframe.addEventListener("load", cleanup, { once: true });
          }}
        />
      </div>

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
                  className={`btn ghost small ${toolbar.overlaps ? "danger" : ""}`}
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
