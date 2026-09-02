import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
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
import { collectUsedFonts, type UsedFontReport } from "./usedFonts";

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
  fontScale?: number;
};

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
  const wheelLock = useRef(false);

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

  useEffect(() => {
    relayout(true);
  }, [fontScale]);

  const turn = (dir: -1 | 1) => {
    const result = goPage(dir);
    if (result === "before") onNeedChapterRef.current(-1);
    if (result === "after") onNeedChapterRef.current(1);
  };

  return (
    <div className="flow-host" ref={hostRef}>
      <div
        className="flow-container"
        ref={containerRef}
        onClick={(e) => {
          const doc = iframeRef.current?.contentDocument;
          if (doc?.getSelection()?.toString()) return;
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

            const paint = (keep: boolean) => {
              if (!live()) return;
              relayout(keep);
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

            const ro = new ResizeObserver(() => {
              if (!live()) return;
              paint(true);
            });
            ro.observe(box);
            if (hostRef.current) ro.observe(hostRef.current);

            const cleanup = () => {
              ro.disconnect();
              doc.removeEventListener("wheel", onDocWheel);
            };
            iframe.addEventListener("load", cleanup, { once: true });
          }}
        />
      </div>
    </div>
  );
});

export default ChapterFrame;
