import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ChapterFrame, {
  type ChapterFrameHandle,
  type PageInfo,
} from "./ChapterFrame";
import FontPanel from "./FontPanel";
import HighlightsPanel from "./HighlightsPanel";
import Library from "./Library";
import TocPanel from "./TocPanel";
import {
  chapterIndex,
  normHref,
  type ChapterPayload,
  type FontSettings,
  type FontSlotId,
  type HighlightRecord,
  type LibraryEntry,
  type OpenedBook,
  type PublisherFontReport,
  type UsedFontReport,
} from "./types";
import { specifiedFamiliesFromReport } from "./usedFonts";
import type { HighlightAnchor } from "./highlights";
import {
  isAppFullscreen,
  setAppFullscreen,
  toggleAppFullscreen,
} from "./fullscreen";

export default function App() {
  const [book, setBook] = useState<OpenedBook | null>(null);
  const [library, setLibrary] = useState<LibraryEntry[]>([]);
  const [resourceOrigin, setResourceOrigin] = useState("");
  const [index, setIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [chapterHtml, setChapterHtml] = useState("");
  const [restoreFraction, setRestoreFraction] = useState(0);
  const [fonts, setFonts] = useState<FontSettings | null>(null);
  const [fontOpen, setFontOpen] = useState(false);
  const [tocOpen, setTocOpen] = useState(false);
  const [highlightsOpen, setHighlightsOpen] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [chromeOn, setChromeOn] = useState(false);
  const fullscreenRef = useRef(false);
  const tocOpenRef = useRef(false);
  const fontOpenRef = useRef(false);
  const highlightsOpenRef = useRef(false);
  const hideChromeTimer = useRef<number | null>(null);
  fullscreenRef.current = fullscreen;
  tocOpenRef.current = tocOpen;
  fontOpenRef.current = fontOpen;
  highlightsOpenRef.current = highlightsOpen;
  const [settingsRev, setSettingsRev] = useState(0);
  const [publisherFonts, setPublisherFonts] = useState<PublisherFontReport | null>(
    null,
  );
  const [usedFonts, setUsedFonts] = useState<UsedFontReport | null>(null);
  /** Highlights of the open book (filtered per chapter when handed to the frame). */
  const [highlights, setHighlights] = useState<HighlightRecord[]>([]);
  /** Highlight to jump to (set by the list; cleared once the frame locates it). */
  const [pendingHighlight, setPendingHighlight] = useState<HighlightRecord | null>(
    null,
  );
  const [pageInfo, setPageInfo] = useState<PageInfo>({
    page: 0,
    pages: 1,
    columns: 1,
  });
  const frameRef = useRef<ChapterFrameHandle>(null);

  const bookRef = useRef(book);
  bookRef.current = book;
  const indexRef = useRef(index);
  indexRef.current = index;
  const pending = useRef<{ key: string; href: string; fraction: number } | null>(
    null,
  );
  const lastFraction = useRef(0);
  const timer = useRef<number | null>(null);

  const spine = book?.spine ?? [];
  const current = spine[index];

  /** Highlights belonging to the currently displayed chapter. */
  const chapterHighlights = useMemo(() => {
    if (!current) return [];
    const exact = normHref(current.href, true);
    return highlights.filter((h) => normHref(h.href, true) === exact);
  }, [highlights, current]);

  /** All highlights in reading order: spine order, then text position. */
  const sortedHighlights = useMemo(() => {
    const order = new Map<string, number>();
    spine.forEach((item, i) => order.set(normHref(item.href, true), i));
    const rank = (h: HighlightRecord) =>
      order.get(normHref(h.href, true)) ?? Number.MAX_SAFE_INTEGER;
    return [...highlights].sort(
      (a, b) =>
        rank(a) - rank(b) ||
        a.startText - b.startText ||
        a.startOffset - b.startOffset,
    );
  }, [highlights, spine]);

  const createHighlight = useCallback(async (href: string, anchor: HighlightAnchor) => {
    const b = bookRef.current;
    if (!b) return;
    try {
      const rec = await invoke<HighlightRecord>("add_annotation", {
        key: b.progressKey,
        href,
        startText: anchor.start.seq,
        startOffset: anchor.start.offset,
        endText: anchor.end.seq,
        endOffset: anchor.end.offset,
        text: anchor.text,
      });
      setHighlights((prev) => [...prev, rec]);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const deleteHighlight = useCallback(async (id: string) => {
    const b = bookRef.current;
    if (!b) return;
    try {
      await invoke("delete_annotation", { key: b.progressKey, id });
      setHighlights((prev) => prev.filter((h) => h.id !== id));
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const flushProgress = useCallback(async () => {
    const next = pending.current;
    if (!next) return;
    pending.current = null;
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    try {
      await invoke("save_progress", {
        key: next.key,
        href: next.href,
        fraction: next.fraction,
      });
    } catch {
      /* keep reading if persist fails */
    }
  }, []);

  const persistReadingPosition = useCallback(async () => {
    const currentBook = bookRef.current;
    const href = currentBook?.spine[indexRef.current]?.href;
    if (currentBook && href) {
      pending.current = {
        key: currentBook.progressKey,
        href,
        fraction: lastFraction.current,
      };
    }
    await flushProgress();
  }, [flushProgress]);

  const queueProgress = useCallback(
    (fraction: number) => {
      const currentBook = bookRef.current;
      const href = currentBook?.spine[indexRef.current]?.href;
      if (!currentBook || !href) return;
      lastFraction.current = fraction;
      pending.current = {
        key: currentBook.progressKey,
        href,
        fraction,
      };
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(flushProgress, 450);
    },
    [flushProgress],
  );

  useEffect(() => {
    const onHide = () => flushProgress();
    window.addEventListener("beforeunload", onHide);
    window.addEventListener("pagehide", onHide);
    const onVis = () => {
      if (document.visibilityState === "hidden") onHide();
    };
    document.addEventListener("visibilitychange", onVis);
    return () => {
      window.removeEventListener("beforeunload", onHide);
      window.removeEventListener("pagehide", onHide);
      document.removeEventListener("visibilitychange", onVis);
      onHide();
    };
  }, [flushProgress]);

  useEffect(() => {
    if (!book || !current) return;
    pending.current = {
      key: book.progressKey,
      href: current.href,
      fraction: restoreFraction,
    };
    flushProgress();
  }, [book, current, restoreFraction, flushProgress]);

  useEffect(() => {
    if (!book || !current) {
      setChapterHtml("");
      setPublisherFonts(null);
      setUsedFonts(null);
      return;
    }
    let cancelled = false;
    invoke<ChapterPayload>("get_chapter", { id: book.id, href: current.href })
      .then((chapter) => {
        if (!cancelled) {
          setChapterHtml(chapter.html);
          setPublisherFonts(chapter.publisherFonts);
        }
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [book, current, settingsRev]);

  const applyFonts = useCallback((next: FontSettings) => {
    setFonts(next);
    setRestoreFraction(lastFraction.current);
    setSettingsRev((n) => n + 1);
  }, []);

  useEffect(() => {
    invoke<FontSettings>("get_font_settings")
      .then(setFonts)
      .catch(() =>
        setFonts({
          useOriginalFonts: true,
          fonts: { serif: null, sans: null, mono: null, cjk: null },
          missingSlots: ["serif", "sans", "mono", "cjk"],
          customFontsActive: false,
          fontScale: 100,
        }),
      );
  }, []);

  const bumpFontScale = useCallback(
    async (delta: number) => {
      const current = fonts?.fontScale ?? 100;
      const nextScale = Math.min(160, Math.max(80, current + delta));
      if (nextScale === current) return;
      try {
        const next = await invoke<FontSettings>("set_font_scale", {
          fontScale: nextScale,
        });
        setFonts(next);
        setRestoreFraction(lastFraction.current);
      } catch (err) {
        setError(String(err));
      }
    },
    [fonts],
  );

  const toggleOriginalFonts = useCallback(
    async (useOriginalFonts: boolean) => {
      try {
        const next = await invoke<FontSettings>("set_use_original_fonts", {
          useOriginalFonts,
        });
        applyFonts(next);
      } catch (err) {
        setError(String(err));
      }
    },
    [applyFonts],
  );

  const uploadFont = useCallback(
    async (slot: FontSlotId) => {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "字体",
            extensions: ["ttf", "otf", "woff", "woff2", "ttc"],
          },
        ],
      });
      if (!selected || Array.isArray(selected)) return;
      try {
        const next = await invoke<FontSettings>("install_font", {
          slot,
          path: selected,
        });
        applyFonts(next);
      } catch (err) {
        setError(String(err));
      }
    },
    [applyFonts],
  );

  const clearFont = useCallback(
    async (slot: FontSlotId) => {
      try {
        const next = await invoke<FontSettings>("clear_font", { slot });
        applyFonts(next);
      } catch (err) {
        setError(String(err));
      }
    },
    [applyFonts],
  );

  const loadLibrary = useCallback(async () => {
    try {
      const entries = await invoke<LibraryEntry[]>("list_library");
      setLibrary(entries);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const goShelf = useCallback(async () => {
    await persistReadingPosition();
    const currentBook = bookRef.current;
    if (currentBook) {
      await invoke("close_book", { id: currentBook.id }).catch(() => undefined);
    }
    setBook(null);
    setChapterHtml("");
    setPublisherFonts(null);
    setUsedFonts(null);
    setHighlights([]);
    setTocOpen(false);
    setHighlightsOpen(false);
    setPendingHighlight(null);
    await loadLibrary();
  }, [persistReadingPosition, loadLibrary]);

  const openPath = useCallback(
    async (selected: string) => {
      await persistReadingPosition();
      setError(null);
      setBusy(true);
      try {
        const currentBook = bookRef.current;
        if (currentBook) {
          await invoke("close_book", { id: currentBook.id }).catch(() => undefined);
        }
        const opened = await invoke<OpenedBook>("open_book", { path: selected });
        setHighlights([]);
        void invoke<HighlightRecord[]>("list_annotations", { key: opened.progressKey })
          .then((list) => {
            // Ignore stale results when another book was opened meanwhile.
            if (bookRef.current?.id === opened.id) setHighlights(list);
          })
          .catch(() => {
            if (bookRef.current?.id === opened.id) setHighlights([]);
          });
        const restored = chapterIndex(opened.spine, opened.progress?.href);
        setBook(opened);
        setIndex(restored >= 0 ? restored : 0);
        const frac =
          restored >= 0 && opened.progress ? opened.progress.fraction : 0;
        lastFraction.current = frac;
        setRestoreFraction(frac);
        setChapterHtml("");
        setPublisherFonts(null);
        setUsedFonts(null);
        setTocOpen(false);
        setHighlightsOpen(false);
        setPendingHighlight(null);
        void loadLibrary();
      } catch (err) {
        setError(String(err));
        setBook(null);
        setTocOpen(false);
        setHighlightsOpen(false);
        setPendingHighlight(null);
        void loadLibrary();
      } finally {
        setBusy(false);
      }
    },
    [persistReadingPosition, loadLibrary],
  );

  const openEpub = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "EPUB", extensions: ["epub"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    await openPath(selected);
  }, [openPath]);

  useEffect(() => {
    invoke<string>("resource_origin")
      .then(setResourceOrigin)
      .catch(() => undefined);
    invoke<string | null>("pending_book")
      .then((path) => {
        if (path) void openPath(path);
        else void loadLibrary();
      })
      .catch(() => {
        void loadLibrary();
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const goChapter = useCallback(
    (delta: number, fraction = 0) => {
      const len = bookRef.current?.spine.length ?? 0;
      const i = indexRef.current;
      const next = Math.min(len - 1, Math.max(0, i + delta));
      if (next === i) return;
      void persistReadingPosition().then(() => {
        lastFraction.current = fraction;
        setRestoreFraction(fraction);
        setIndex(next);
      });
    },
    [persistReadingPosition],
  );

  const goPage = useCallback(
    (delta: number) => {
      const result = frameRef.current?.goPage(delta);
      if (result === "after") goChapter(1, 0);
      if (result === "before") goChapter(-1, 1);
    },
    [goChapter],
  );

  const goToHref = useCallback(
    (href: string) => {
      const items = bookRef.current?.spine ?? [];
      const i = chapterIndex(items, href);
      if (i < 0) return;
      if (i === indexRef.current) {
        lastFraction.current = 0;
        setRestoreFraction(0);
        frameRef.current?.goToPage(0);
        return;
      }
      flushProgress();
      lastFraction.current = 0;
      setRestoreFraction(0);
      setIndex(i);
    },
    [flushProgress],
  );

  const goToHighlight = useCallback(
    (rec: HighlightRecord) => {
      setHighlightsOpen(false);
      const items = bookRef.current?.spine ?? [];
      const i = chapterIndex(items, rec.href);
      if (i < 0) return;
      setPendingHighlight(rec);
      if (i === indexRef.current) return;
      flushProgress();
      lastFraction.current = 0;
      setRestoreFraction(0);
      setIndex(i);
    },
    [flushProgress],
  );

  const onHighlightLocated = useCallback(() => setPendingHighlight(null), []);

  const showChrome = useCallback(() => {
    if (hideChromeTimer.current !== null) {
      window.clearTimeout(hideChromeTimer.current);
      hideChromeTimer.current = null;
    }
    setChromeOn(true);
  }, []);

  const scheduleHideChrome = useCallback(() => {
    if (fontOpenRef.current || tocOpenRef.current || highlightsOpenRef.current) {
      return;
    }
    if (hideChromeTimer.current !== null) {
      window.clearTimeout(hideChromeTimer.current);
    }
    hideChromeTimer.current = window.setTimeout(() => {
      setChromeOn(false);
      hideChromeTimer.current = null;
    }, 280);
  }, []);

  const toggleFullscreen = useCallback(async () => {
    try {
      const next = await toggleAppFullscreen();
      setFullscreen(next);
      setChromeOn(false);
    } catch {
      const on = await isAppFullscreen().catch(() => fullscreenRef.current);
      setFullscreen(on);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void isAppFullscreen()
      .then((on) => {
        if (!cancelled) setFullscreen(on);
      })
      .catch(() => undefined);
    void import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) =>
        getCurrentWindow().onResized(async () => {
          try {
            const on = await isAppFullscreen();
            if (!cancelled) setFullscreen(on);
          } catch {
            /* ignore */
          }
        }),
      )
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (fullscreen && (fontOpen || tocOpen || highlightsOpen)) setChromeOn(true);
    if (!fullscreen) setChromeOn(false);
  }, [fullscreen, fontOpen, tocOpen, highlightsOpen]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "F11" || e.key === "F11") {
        e.preventDefault();
        e.stopImmediatePropagation();
        void toggleFullscreen();
        return;
      }
      if (e.key === "Escape") {
        if (highlightsOpenRef.current) {
          setHighlightsOpen(false);
          return;
        }
        if (tocOpenRef.current) {
          setTocOpen(false);
          return;
        }
        if (fullscreenRef.current) {
          e.preventDefault();
          void setAppFullscreen(false)
            .then((on) => setFullscreen(on))
            .catch(() => setFullscreen(false));
          return;
        }
        return;
      }
      if (e.key === "ArrowRight" || e.key === "PageDown") {
        e.preventDefault();
        goPage(1);
      }
      if (e.key === "ArrowLeft" || e.key === "PageUp") {
        e.preventDefault();
        goPage(-1);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [goPage, toggleFullscreen]);

  return (
    <div
      className={[
        "shell",
        fullscreen ? "fullscreen" : "",
        fullscreen && (chromeOn || fontOpen || tocOpen) ? "chrome-on" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {fullscreen && (
        <div
          className="chrome-hotzone"
          onMouseEnter={showChrome}
        />
      )}
      <header
        className="chrome"
        onMouseEnter={showChrome}
        onMouseLeave={scheduleHideChrome}
      >
        <div className="brand">IcedReader</div>
        {book && (
          <button
            type="button"
            className="btn ghost"
            onClick={() => void goShelf()}
            disabled={busy}
          >
            书架
          </button>
        )}
        <button type="button" className="btn" onClick={openEpub} disabled={busy}>
          {busy ? "打开中…" : "打开 EPUB"}
        </button>
        <button
          type="button"
          className="btn ghost"
          onClick={() => {
            setTocOpen((openNow) => !openNow);
            setHighlightsOpen(false);
          }}
          disabled={!book}
        >
          目录
        </button>
        <button
          type="button"
          className="btn ghost"
          onClick={() => {
            setHighlightsOpen((openNow) => !openNow);
            setTocOpen(false);
          }}
          disabled={!book}
        >
          划线
        </button>
        <button
          type="button"
          className="btn ghost"
          onClick={() => setFontOpen((openNow) => !openNow)}
        >
          字体
        </button>
        <div className="type-size">
          <button
            type="button"
            className="btn ghost small"
            disabled={!fonts || (fonts.fontScale ?? 100) <= 80}
            onClick={() => void bumpFontScale(-10)}
            title="缩小字号"
          >
            A−
          </button>
          <span>{fonts?.fontScale ?? 100}%</span>
          <button
            type="button"
            className="btn ghost small"
            disabled={!fonts || (fonts.fontScale ?? 100) >= 160}
            onClick={() => void bumpFontScale(10)}
            title="放大字号"
          >
            A+
          </button>
        </div>
        <button
          type="button"
          className="btn ghost"
          onClick={() => void toggleFullscreen()}
          title="F11"
        >
          {fullscreen ? "退出全屏" : "全屏"}
        </button>
        {book && (
          <>
            <div className="meta" title={book.metadata.title}>
              <strong>{book.metadata.title}</strong>
              <span>
                {book.metadata.authors.length
                  ? book.metadata.authors.join("、")
                  : "未知作者"}
              </span>
            </div>
            <div className="nav">
              <button
                type="button"
                className="btn ghost"
                onClick={() => goChapter(-1, 0)}
                disabled={index <= 0}
              >
                上一章
              </button>
              <span
                className="pos"
                title={current?.title ?? current?.href ?? ""}
              >
                {current?.title ? `${current.title} · ` : ""}
                {spine.length ? `${index + 1}/${spine.length}章` : "0章"}
                {` · ${pageInfo.page + 1}/${pageInfo.pages}页`}
                {pageInfo.columns === 2 ? " · 双栏" : ""}
              </span>
              <button
                type="button"
                className="btn ghost"
                onClick={() => goChapter(1, 0)}
                disabled={index >= spine.length - 1}
              >
                下一章
              </button>
            </div>
          </>
        )}
      </header>

      {fontOpen && fonts && (
        <FontPanel
          settings={fonts}
          publisherFonts={publisherFonts}
          usedFonts={usedFonts}
          busy={busy}
          onToggleOriginal={toggleOriginalFonts}
          onUpload={uploadFont}
          onClear={clearFont}
        />
      )}

      {error && <div className="banner">{error}</div>}
      {book && fonts && !fonts.useOriginalFonts && !fonts.customFontsActive && (
        <div className="banner">自定义字体未齐，当前仍按原书 CSS。</div>
      )}

      <div className="workspace">
        {tocOpen && book && (
          <>
            <button
              type="button"
              className="toc-dim"
              aria-label="关闭目录"
              onClick={() => setTocOpen(false)}
            />
            <TocPanel
              toc={book.toc}
              spine={spine}
              currentIndex={index}
              onSelect={goToHref}
              onClose={() => setTocOpen(false)}
            />
          </>
        )}
        {highlightsOpen && book && (
          <>
            <button
              type="button"
              className="toc-dim"
              aria-label="关闭划线"
              onClick={() => setHighlightsOpen(false)}
            />
            <HighlightsPanel
              highlights={sortedHighlights}
              spine={spine}
              currentHref={current?.href ?? ""}
              onSelect={goToHighlight}
              onDelete={(id) => void deleteHighlight(id)}
              onClose={() => setHighlightsOpen(false)}
            />
          </>
        )}
        <main className="stage">
          {!book && (
            <Library
              entries={library}
              origin={resourceOrigin}
              busy={busy}
              onOpen={(path) => void openPath(path)}
              onImport={() => void openEpub()}
            />
          )}
          {book && chapterHtml && (
            <div className="page">
              <ChapterFrame
                ref={frameRef}
                html={chapterHtml}
                restoreFraction={restoreFraction}
                fontScale={fonts?.fontScale ?? 100}
                documentLang={book.metadata.language}
                authorFamilies={specifiedFamiliesFromReport(
                  publisherFonts?.declarations.map((d) => d.value) ?? [],
                  publisherFonts?.faces ?? [],
                )}
                highlights={chapterHighlights}
                chapterHref={current.href}
                onCreateHighlight={createHighlight}
                onDeleteHighlight={deleteHighlight}
                pendingHighlight={pendingHighlight}
                onHighlightLocated={onHighlightLocated}
                onProgress={queueProgress}
                onUsedFonts={setUsedFonts}
                onPageInfo={setPageInfo}
                onNeedChapter={(delta) => goChapter(delta, delta < 0 ? 1 : 0)}
              />
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
