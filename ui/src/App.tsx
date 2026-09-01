import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import ChapterFrame, {
  type ChapterFrameHandle,
  type PageInfo,
} from "./ChapterFrame";
import FontPanel from "./FontPanel";
import TocPanel from "./TocPanel";
import {
  chapterIndex,
  type ChapterPayload,
  type FontSettings,
  type FontSlotId,
  type OpenedBook,
  type PublisherFontReport,
  type UsedFontReport,
} from "./types";
import { specifiedFamiliesFromReport } from "./usedFonts";
import {
  isAppFullscreen,
  setAppFullscreen,
  toggleAppFullscreen,
} from "./fullscreen";

export default function App() {
  const [book, setBook] = useState<OpenedBook | null>(null);
  const [index, setIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [chapterHtml, setChapterHtml] = useState("");
  const [restoreFraction, setRestoreFraction] = useState(0);
  const [fonts, setFonts] = useState<FontSettings | null>(null);
  const [fontOpen, setFontOpen] = useState(false);
  const [tocOpen, setTocOpen] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [chromeOn, setChromeOn] = useState(false);
  const fullscreenRef = useRef(false);
  const tocOpenRef = useRef(false);
  const fontOpenRef = useRef(false);
  const hideChromeTimer = useRef<number | null>(null);
  fullscreenRef.current = fullscreen;
  tocOpenRef.current = tocOpen;
  fontOpenRef.current = fontOpen;
  const [settingsRev, setSettingsRev] = useState(0);
  const [publisherFonts, setPublisherFonts] = useState<PublisherFontReport | null>(
    null,
  );
  const [usedFonts, setUsedFonts] = useState<UsedFontReport | null>(null);
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

  const flushProgress = useCallback(() => {
    const next = pending.current;
    if (!next) return;
    pending.current = null;
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    void invoke("save_progress", {
      key: next.key,
      href: next.href,
      fraction: next.fraction,
    }).catch(() => undefined);
  }, []);

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
        }),
      );
  }, []);

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

  const openPath = useCallback(
    async (selected: string) => {
      flushProgress();
      setError(null);
      setBusy(true);
      try {
        const currentBook = bookRef.current;
        if (currentBook) {
          await invoke("close_book", { id: currentBook.id }).catch(() => undefined);
        }
        const opened = await invoke<OpenedBook>("open_book", { path: selected });
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
      } catch (err) {
        setError(String(err));
        setBook(null);
        setTocOpen(false);
      } finally {
        setBusy(false);
      }
    },
    [flushProgress],
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
    invoke<string | null>("pending_book")
      .then((path) => {
        if (path) void openPath(path);
      })
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const goChapter = useCallback(
    (delta: number, fraction = 0) => {
      const len = bookRef.current?.spine.length ?? 0;
      const i = indexRef.current;
      const next = Math.min(len - 1, Math.max(0, i + delta));
      if (next === i) return;
      flushProgress();
      lastFraction.current = fraction;
      setRestoreFraction(fraction);
      setIndex(next);
    },
    [flushProgress],
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

  const showChrome = useCallback(() => {
    if (hideChromeTimer.current !== null) {
      window.clearTimeout(hideChromeTimer.current);
      hideChromeTimer.current = null;
    }
    setChromeOn(true);
  }, []);

  const scheduleHideChrome = useCallback(() => {
    if (fontOpenRef.current || tocOpenRef.current) return;
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
    if (fullscreen && (fontOpen || tocOpen)) setChromeOn(true);
    if (!fullscreen) setChromeOn(false);
  }, [fullscreen, fontOpen, tocOpen]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "F11" || e.key === "F11") {
        e.preventDefault();
        e.stopImmediatePropagation();
        void toggleFullscreen();
        return;
      }
      if (e.key === "Escape") {
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
        <button type="button" className="btn" onClick={openEpub} disabled={busy}>
          {busy ? "打开中…" : "打开 EPUB"}
        </button>
        <button
          type="button"
          className="btn ghost"
          onClick={() => setTocOpen((openNow) => !openNow)}
          disabled={!book}
        >
          目录
        </button>
        <button
          type="button"
          className="btn ghost"
          onClick={() => setFontOpen((openNow) => !openNow)}
        >
          字体
        </button>
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
        <main className="stage">
          {!book && (
            <div className="empty">
              <p>打开一本 EPUB，开始阅读。</p>
              <p className="hint">进度按章节和页序保存，换窗口宽也能对上。</p>
            </div>
          )}
          {book && chapterHtml && (
            <div className="page">
              <ChapterFrame
                ref={frameRef}
                html={chapterHtml}
                restoreFraction={restoreFraction}
                documentLang={book.metadata.language}
                authorFamilies={specifiedFamiliesFromReport(
                  publisherFonts?.declarations.map((d) => d.value) ?? [],
                  publisherFonts?.faces ?? [],
                )}
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
