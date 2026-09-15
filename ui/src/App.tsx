import { ask, open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ChapterFrame, {
  type ChapterFrameHandle,
  type PageInfo,
} from "./ChapterFrame";
import BookMetaPanel from "./BookMetaPanel";
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
import PdfView, { type PdfViewHandle, type PdfViewState } from "./PdfView";
import type { PdfSpread } from "./pdfPaging";
import {
  isAppFullscreen,
  setAppFullscreen,
  toggleAppFullscreen,
} from "./fullscreen";

/** 库内 epub 文件名（书命令与 notes.md 档案用）。 */
function fileNameOf(b: OpenedBook): string {
  return b.path.split(/[\\/]/).pop() ?? "";
}

export default function App() {
  const [book, setBook] = useState<OpenedBook | null>(null);
  const [library, setLibrary] = useState<LibraryEntry[]>([]);
  /** 书架三点菜单「编辑元数据…」选中的条目（非 null 时显示模态面板）。 */
  const [metaEntry, setMetaEntry] = useState<LibraryEntry | null>(null);
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
  /** Narrow-window overflow menu in the top bar (reading view only). */
  const [topMenuOpen, setTopMenuOpen] = useState(false);
  /** 「跳到全书位置」输入浮层。 */
  const [jumpOpen, setJumpOpen] = useState(false);
  const [jumpValue, setJumpValue] = useState("");
  const fullscreenRef = useRef(false);
  const tocOpenRef = useRef(false);
  const fontOpenRef = useRef(false);
  const highlightsOpenRef = useRef(false);
  const topMenuOpenRef = useRef(false);
  const hideChromeTimer = useRef<number | null>(null);
  fullscreenRef.current = fullscreen;
  tocOpenRef.current = tocOpen;
  fontOpenRef.current = fontOpen;
  highlightsOpenRef.current = highlightsOpen;
  topMenuOpenRef.current = topMenuOpen;
  const [settingsRev, setSettingsRev] = useState(0);
  const [publisherFonts, setPublisherFonts] = useState<PublisherFontReport | null>(
    null,
  );
  const [usedFonts, setUsedFonts] = useState<UsedFontReport | null>(null);
  /** Highlights of the open book (filtered per chapter when handed to the frame). */
  const [highlights, setHighlights] = useState<HighlightRecord[]>([]);
  /** 备注正文（notes.md 用户区）id → 文本；hover 浮层与划线列表的数据源。 */
  const [notesById, setNotesById] = useState<Record<string, string>>({});
  const notesRef = useRef(notesById);
  notesRef.current = notesById;
  /** Highlight to jump to (set by the list; cleared once the frame locates it). */
  const [pendingHighlight, setPendingHighlight] = useState<HighlightRecord | null>(
    null,
  );
  const [pendingFragment, setPendingFragment] = useState<string | null>(null);
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
  const isPdfRef = useRef(false);
  const pending = useRef<{ key: string; href: string; fraction: number } | null>(
    null,
  );
  const lastFraction = useRef(0);
  const timer = useRef<number | null>(null);

  const spine = book?.spine ?? [];
  const current = spine[index];
  /** PDF：一页 = 一个 spine 单元。字号/字体/划线对 PDF 无意义，改由缩放接管。 */
  const isPdf = book?.format === "pdf";
  /** PDF 缩放：适应页面（纸高＝阅读区高）/ 适应宽度（纸宽＝阅读区宽）。 */
  const [pdfFit, setPdfFit] = useState<"page" | "width">("page");
  /** 适应页面时的张数：自动（默认，按真实页面宽高比判）/ 单页 / 双页。 */
  const [pdfSpread, setPdfSpread] = useState<PdfSpread>("auto");
  /** PDF 纸带上报的当前状态：页码、实际张数、是否到头（顶栏禁用态与进度都靠它）。 */
  const [pdfState, setPdfState] = useState<PdfViewState | null>(null);
  /** 打开时的格式提示条（PDF 缺字风险）是否已被关掉。 */
  const [warnDismissed, setWarnDismissed] = useState(false);
  useEffect(() => {
    setWarnDismissed(false);
    // 换书时旧纸带的上报作废（新 PdfView 会立刻重新上报），否则顶栏禁用态会短暂用旧值。
    setPdfState(null);
  }, [book?.id]);

  isPdfRef.current = !!isPdf;
  /** 连续纸带视图的把手（上一页/下一页、跳页、滚一屏）。 */
  const pdfRef = useRef<PdfViewHandle>(null);
  /** 帧（EPUB）与纸带（PDF）各自上报的页码；换书时都作废。 */
  const onPdfState = useCallback(
    (state: PdfViewState) => {
      setPdfState(state);
      const next = state.page - 1;
      if (next >= 0 && next !== indexRef.current) setIndex(next);
    },
    [],
  );
  /** EPUB 的取文档 key（spine href）。PDF 不走 `get_chapter`——连续纸带直接拼图片
   *  URL（见 PdfView.tsx），所以这里是空串、取文档 effect 会跳过。 */
  const requestHref = useMemo(
    () => (isPdf ? "" : current?.href ?? ""),
    [isPdf, current?.href],
  );
  /** 每章在全书的起始字符偏移（前缀和）+ 总字符；划线 pos 与「全书%」跳转共用。 */
  const bookCum = useMemo(() => {
    const chars = book?.chapterChars ?? [];
    const cum = new Array<number>(chars.length + 1);
    let acc = 0;
    for (let i = 0; i < chars.length; i++) {
      cum[i] = acc;
      acc += chars[i];
    }
    cum[chars.length] = acc;
    return cum;
  }, [book?.chapterChars]);
  const bookPos =
    bookCum.length > 1 && bookCum[bookCum.length - 1] > 0
      ? { start: bookCum[Math.min(index, bookCum.length - 2)], total: bookCum[bookCum.length - 1] }
      : undefined;
  /** 当前阅读位置的全书百分比（页比例近似章内字符比例，用于显示与跳转）。 */
  const bookPercent = (() => {
    const total = bookCum.length > 1 ? bookCum[bookCum.length - 1] : 0;
    if (total <= 0) return null;
    const curChars = book?.chapterChars?.[index] ?? 0;
    const frac =
      pageInfo.pages > 1 ? pageInfo.page / (pageInfo.pages - 1) : 0;
    return Math.round(
      ((bookCum[index] + frac * curChars) / total) * 100,
    );
  })();


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

  const createHighlight = useCallback(
    async (href: string, anchor: HighlightAnchor, color: string, pos: number) => {
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
          color,
          pos,
        });
        setHighlights((prev) => [...prev, rec]);
      } catch (err) {
        setError(String(err));
      }
    },
    [],
  );

  const deleteHighlight = useCallback(async (id: string) => {
    const b = bookRef.current;
    if (!b) return;
    const note = notesRef.current[id];
    if (note !== undefined) {
      const preview = note.length > 60 ? `${note.slice(0, 60)}…` : note;
      const ok = window.confirm(
        `这条划线有备注：\n「${preview}」\n\n删除后正文高亮消失；划线内容与备注保留在 notes.md 并记删除时间。\n确定删除这条划线？`,
      );
      if (!ok) return;
    }
    try {
      await invoke("delete_annotation", {
        fileName: fileNameOf(b),
        key: b.progressKey,
        id,
      });
      setHighlights((prev) => prev.filter((h) => h.id !== id));
      setNotesById((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    } catch (err) {
      setError(String(err));
    }
  }, []);

  /** 写/清一条划线的备注（notes.md 是备注的唯一真相源）。 */
  const saveNote = useCallback(async (id: string, note: string) => {
    const b = bookRef.current;
    if (!b) return;
    const cleaned = note.trim();
    try {
      await invoke("save_note", {
        fileName: fileNameOf(b),
        bookId: b.id,
        key: b.progressKey,
        id,
        note: cleaned,
      });
      setNotesById((prev) => {
        const next = { ...prev };
        if (cleaned) next[id] = cleaned;
        else delete next[id];
        return next;
      });
    } catch (err) {
      setError(String(err));
    }
  }, []);

  /** 读回 notes.md 的备注（打开书后调用；书外改过的备注在此刷新）。 */
  const notesGen = useRef(0);
  const loadNotes = useCallback(async (fileName: string) => {
    const gen = ++notesGen.current;
    try {
      const list = await invoke<{ id: string; note: string }[]>("read_notes", {
        fileName,
      });
      if (gen !== notesGen.current) return;
      const map: Record<string, string> = {};
      for (const x of list) map[x.id] = x.note;
      setNotesById(map);
    } catch {
      if (gen !== notesGen.current) return;
      setNotesById({});
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
    if (!topMenuOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setTopMenuOpen(false);
    };
    const onDown = (e: PointerEvent) => {
      const t = e.target as Element | null;
      if (!t?.closest?.(".top-more")) setTopMenuOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onDown);
    };
  }, [topMenuOpen]);

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
    if (!book) {
      setChapterHtml("");
      setPublisherFonts(null);
      setUsedFonts(null);
      return;
    }
    // 只有 `requestHref` 变了才重新取文档；EPUB 的它就是 spine href。
    // PDF 的 `requestHref` 恒为空串 —— 连续纸带由 `PdfView` 直接拼图片 URL，
    // 不走 `get_chapter`，所以滚到哪一页都不会触发文档请求。
    if (!requestHref) return;
    let cancelled = false;
    invoke<ChapterPayload>("get_chapter", { id: book.id, href: requestHref })
      .then((chapter) => {
        if (!cancelled) {
          setChapterHtml(chapter.html);
          setPublisherFonts(chapter.publisherFonts);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setChapterHtml("");
          setPublisherFonts(null);
          setUsedFonts(null);
          setError(String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [book, requestHref, settingsRev]);

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

  const deleteBook = useCallback(
    async (entry: LibraryEntry) => {
      const confirmed = await ask(
        `确定从书库删除《${entry.title}》吗？\n将同时删除这本书的阅读进度和划线。`,
        {
          title: "删除书籍",
          kind: "warning",
          okLabel: "删除",
          cancelLabel: "取消",
        },
      );
      if (!confirmed) return;
      setBusy(true);
      try {
        await invoke("delete_book", {
          fileName: entry.fileName,
          progressKey: entry.progressKey,
        });
        await loadLibrary();
      } catch (err) {
        setError(String(err));
      } finally {
        setBusy(false);
      }
    },
    [loadLibrary],
  );

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
        setNotesById({});
        void invoke<HighlightRecord[]>("list_annotations", { key: opened.progressKey })
          .then((list) => {
            // Ignore stale results when another book was opened meanwhile.
            if (bookRef.current?.id === opened.id) setHighlights(list);
          })
          .catch(() => {
            if (bookRef.current?.id === opened.id) setHighlights([]);
          });
        void loadNotes(fileNameOf(opened));
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
    [persistReadingPosition, loadLibrary, loadNotes],
  );

  /** 导入一本外部电子书（EPUB / PDF 都走这一条，格式由 Rust 侧分派）。 */
  const openBook = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "电子书", extensions: ["epub", "pdf"] }],
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
        setChapterHtml("");
        setPublisherFonts(null);
        setUsedFonts(null);
        setIndex(next);
      });
    },
    [persistReadingPosition],
  );

  /**
   * PDF 跳页（目录 / 全书% / 翻页）：先让纸带把目标页滚到视口顶部，再更新 index。
   *
   * index 是**权威**：纸带上报的当前页只用来同步显示与进度，不会反过来驱动滚动——
   * 所以「跳页」与「用户滚动」不会互相打架。进度只记页（fraction 恒 0），页内位置
   * 是临时视图状态。
   */
  const pdfGoTo = useCallback(
    (next: number, scrollFraction = 0) => {
      const len = bookRef.current?.spine.length ?? 0;
      const target = Math.min(len - 1, Math.max(0, next));
      pdfRef.current?.goToPage(target + 1, scrollFraction);
      if (target === indexRef.current) return;
      void persistReadingPosition().then(() => {
        lastFraction.current = 0;
        setRestoreFraction(0);
        setIndex(target);
      });
    },
    [persistReadingPosition],
  );

  /** 章边界续翻（EPUB）：向回翻停在上一章末尾。PDF 由纸带自己按跨页步进。 */
  const stepChapter = useCallback(
    (delta: -1 | 1) => {
      goChapter(delta, delta > 0 ? 0 : 1);
    },
    [goChapter],
  );

  const goPage = useCallback(
    (delta: number) => {
      // PDF：纸带按「行」翻（双页就是跨页）。EPUB：分栏分页器，到边界续章。
      if (isPdfRef.current) {
        pdfRef.current?.goPage(delta < 0 ? -1 : 1);
        return;
      }
      const result = frameRef.current?.goPage(delta);
      if (result === "after") stepChapter(1);
      if (result === "before") stepChapter(-1);
    },
    [stepChapter],
  );

  /** 上一页 / 下一页（PDF）：纸带按行（＝跨页）步进。 */
  const pdfTurn = useCallback((delta: -1 | 1) => {
    pdfRef.current?.goPage(delta);
  }, []);

  /** 到头的按钮要禁用：PDF 用纸带上报的 atStart/atEnd（双页步长不是 1）。 */
  const prevDisabled = isPdf ? (pdfState?.atStart ?? true) : index <= 0;
  const nextDisabled = isPdf
    ? (pdfState?.atEnd ?? true)
    : index >= spine.length - 1;
  const goToHref = useCallback(
    (href: string) => {
      const items = bookRef.current?.spine ?? [];
      const i = chapterIndex(items, href);
      if (i < 0) return;
      const hashAt = href.indexOf("#");
      const frag =
        hashAt >= 0 ? href.slice(hashAt + 1).split("?")[0] : "";
      setPendingFragment(frag || null);
      if (i === indexRef.current) {
        if (frag) {
          frameRef.current?.goToFragment(frag);
        } else if (isPdfRef.current) {
          // 同一页：把纸带挪到这一页顶部就行（PDF 没有页内锚点）。
          pdfRef.current?.goToPage(i + 1, 0);
        } else {
          lastFraction.current = 0;
          setRestoreFraction(0);
          frameRef.current?.goToPage(0);
        }
        return;
      }
      if (isPdfRef.current) {
        pdfGoTo(i, 0);
        return;
      }
      flushProgress();
      lastFraction.current = 0;
      setRestoreFraction(0);
      setChapterHtml("");
      setPublisherFonts(null);
      setUsedFonts(null);
      setIndex(i);
    },
    [flushProgress],
  );

  /** 跳到「全书 N%」：按每章字符权重换算到章 + 章内比例；同章直接定位，
   *  跨章先存当前进度再切章。跳转后按新位置保存进度（与翻章同语义）。 */
  const jumpToBookPercent = useCallback(
    (pct: number) => {
      const b = bookRef.current;
      if (!b || !b.chapterChars.length) return;
      setJumpOpen(false);
      const chars = b.chapterChars;
      const total = chars.reduce((acc, c) => acc + c, 0);
      if (total <= 0) return;
      const target = Math.min(total - 1, Math.max(0, (pct / 100) * total));
      let i = 0;
      let acc = 0;
      while (i < chars.length - 1 && acc + chars[i] <= target) {
        acc += chars[i];
        i += 1;
      }
      const len = chars[i] || 1;
      const frac = Math.min(0.9999, Math.max(0, (target - acc) / len));
      if (i === indexRef.current) {
        if (isPdfRef.current) {
          // PDF：chapterChars 每页 1，frac 就是页内比例 —— 把纸带滚到页内那个位置；
          // 进度仍是「页 + fraction 0」（页内位置属于临时视图状态）。
          pdfRef.current?.goToPage(i + 1, frac);
          return;
        }
        lastFraction.current = frac;
        frameRef.current?.goToFraction(frac);
        return;
      }
      if (isPdfRef.current) {
        pdfGoTo(i, frac);
        return;
      }
      void persistReadingPosition().then(() => {
        lastFraction.current = frac;
        setRestoreFraction(frac);
        setIndex(i);
      });
    },
    [persistReadingPosition, pdfGoTo],
  );

  const confirmJump = () => {
    const n = Number(jumpValue);
    if (Number.isFinite(n)) jumpToBookPercent(Math.min(100, Math.max(0, n)));
  };

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
      // 焦点在可编辑控件（编辑元数据等模态输入框）时不劫持方向键——
      // 左右键用于移动光标而非翻页。F11/Esc 不在此列，保持全局语义。
      const t = e.target as HTMLElement | null;
      const editing = !!(
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.tagName === "SELECT" ||
          t.isContentEditable)
      );
      if (e.code === "F11" || e.key === "F11") {
        e.preventDefault();
        e.stopImmediatePropagation();
        void toggleFullscreen();
        return;
      }
      if (e.key === "Escape") {
        if (editing) return;
        // Layered close: the overflow menu first, then 划线 / 字体 / 目录, then
        // fullscreen (AGENTS: Esc 先关浮层/目录再退出全屏).
        if (topMenuOpenRef.current) {
          setTopMenuOpen(false);
          return;
        }
        if (highlightsOpenRef.current) {
          setHighlightsOpen(false);
          return;
        }
        if (fontOpenRef.current) {
          setFontOpen(false);
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
      if (editing) return;
      if (!bookRef.current) return;
      // PDF：滚动就是滚动 —— 左右键翻页（双页按跨页），PageDown/Space 滚一屏，
      // Home/End 到首/末页。EPUB 的按键语义一字未改（下面那两条老规矩）。
      if (isPdfRef.current) {
        const pageCount = bookRef.current.spine.length;
        const key = e.key;
        if (key === "ArrowRight") {
          e.preventDefault();
          pdfRef.current?.goPage(1);
          return;
        }
        if (key === "ArrowLeft") {
          e.preventDefault();
          pdfRef.current?.goPage(-1);
          return;
        }
        if (key === "PageDown" || (key === " " && !e.shiftKey)) {
          e.preventDefault();
          pdfRef.current?.scrollScreen(1);
          return;
        }
        if (key === "PageUp" || (key === " " && e.shiftKey)) {
          e.preventDefault();
          pdfRef.current?.scrollScreen(-1);
          return;
        }
        if (key === "Home") {
          e.preventDefault();
          pdfRef.current?.goToPage(1);
          return;
        }
        if (key === "End") {
          e.preventDefault();
          pdfRef.current?.goToPage(pageCount);
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
        book ? "reading" : "",
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
        <button
          type="button"
          className="btn chrome-more"
          onClick={openBook}
          disabled={busy}
        >
          {busy ? "打开中…" : "打开电子书"}
        </button>
        <button
          type="button"
          className="btn ghost chrome-more"
          onClick={() => {
            setTocOpen((openNow) => !openNow);
            setHighlightsOpen(false);
          }}
          disabled={!book}
        >
          目录
        </button>
        {!isPdf && (
          <button
            type="button"
            className="btn ghost chrome-more"
            onClick={() => {
              setHighlightsOpen((openNow) => !openNow);
              setTocOpen(false);
            }}
            disabled={!book}
          >
            划线
          </button>
        )}
        {!isPdf && (
          <button
            type="button"
            className="btn ghost chrome-more"
            onClick={() => setFontOpen((openNow) => !openNow)}
          >
            字体
          </button>
        )}
        {isPdf ? (
          <>
            <div className="zoom-mode" role="group" aria-label="PDF 缩放">
              <button
                type="button"
                className={`btn ghost small${pdfFit === "page" ? " on" : ""}`}
                aria-pressed={pdfFit === "page"}
                title="整页落在窗口里"
                onClick={() => setPdfFit("page")}
              >
                适应页面
              </button>
              <button
                type="button"
                className={`btn ghost small${pdfFit === "width" ? " on" : ""}`}
                aria-pressed={pdfFit === "width"}
                title="页宽铺满窗口，连续滚动"
                onClick={() => setPdfFit("width")}
              >
                适应宽度
              </button>
            </div>
            {/* 张数只在「适应页面」下露出：适应宽度就是「纸宽贴合窗口、一次一张」
                （并排阅读用「适应页面 + 双页」，等价且意图明确）。所以适应宽度时
                给纸带的 spread 恒为 single——不然在适应页面选了双页、再切过来，
                会得到控制项看不见却已经并排的两张纸。 */}
            {pdfFit === "page" && (
              <div className="zoom-mode" role="group" aria-label="PDF 张数">
                <button
                  type="button"
                  className={`btn ghost small${pdfSpread === "auto" ? " on" : ""}`}
                  aria-pressed={pdfSpread === "auto"}
                  title="按页面比例自动：横向空间够就并排两页"
                  onClick={() => setPdfSpread("auto")}
                >
                  自动
                </button>
                <button
                  type="button"
                  className={`btn ghost small${pdfSpread === "single" ? " on" : ""}`}
                  aria-pressed={pdfSpread === "single"}
                  title="一次一张纸"
                  onClick={() => setPdfSpread("single")}
                >
                  单页
                </button>
                <button
                  type="button"
                  className={`btn ghost small${pdfSpread === "double" ? " on" : ""}`}
                  aria-pressed={pdfSpread === "double"}
                  title="书式跨页：封面单独，之后 2-3、4-5 成对"
                  onClick={() => setPdfSpread("double")}
                >
                  双页
                </button>
              </div>
            )}
          </>
        ) : (
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
        )}
        <button
          type="button"
          className="btn ghost chrome-more"
          onClick={() => void toggleFullscreen()}
          title="F11"
        >
          {fullscreen ? "退出全屏" : "全屏"}
        </button>
        {book && (
          <div className="top-more">
            <button
              type="button"
              className="btn ghost top-more-btn"
              aria-label="更多操作"
              aria-haspopup="menu"
              aria-expanded={topMenuOpen}
              title="更多操作"
              onClick={() => setTopMenuOpen((openNow) => !openNow)}
            >
              <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
                <circle cx="8" cy="3.2" r="1.7" fill="currentColor" />
                <circle cx="8" cy="8" r="1.7" fill="currentColor" />
                <circle cx="8" cy="12.8" r="1.7" fill="currentColor" />
              </svg>
            </button>
            {topMenuOpen && (
              <div className="top-menu" role="menu">
                <button
                  type="button"
                  role="menuitem"
                  disabled={busy}
                  onClick={() => {
                    setTopMenuOpen(false);
                    void openBook();
                  }}
                >
                  打开电子书
                </button>
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setTopMenuOpen(false);
                    setTocOpen((openNow) => !openNow);
                    setHighlightsOpen(false);
                  }}
                >
                  目录
                </button>
                {!isPdf && (
                  <button
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      setTopMenuOpen(false);
                      setHighlightsOpen((openNow) => !openNow);
                      setTocOpen(false);
                    }}
                  >
                    划线
                  </button>
                )}
                {!isPdf && (
                  <button
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      setTopMenuOpen(false);
                      setFontOpen((openNow) => !openNow);
                    }}
                  >
                    字体
                  </button>
                )}
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setTopMenuOpen(false);
                    void toggleFullscreen();
                  }}
                >
                  {fullscreen ? "退出全屏" : "全屏"}
                </button>
              </div>
            )}
          </div>
        )}
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
                onClick={() => (isPdf ? pdfTurn(-1) : goChapter(-1, 0))}
                disabled={prevDisabled}
              >
                {isPdf ? "上一页" : "上一章"}
              </button>
              <span
                className="pos"
                title={current?.title ?? current?.href ?? ""}
              >
                {current?.title ? `${current.title} · ` : ""}
                {isPdf
                  ? spine.length
                    ? `第 ${index + 1} / ${spine.length} 页${
                        pdfState?.spread === "double" ? " · 跨页" : ""
                      }`
                    : "0 页"
                  : `${spine.length ? `${index + 1}/${spine.length}章` : "0章"} · ${
                      pageInfo.page + 1
                    }/${pageInfo.pages}页${
                      pageInfo.columns === 2 ? " · 双栏" : ""
                    }`}
                {bookPercent !== null && (
                  <button
                    type="button"
                    className="pos-jump"
                    title="跳到全书位置（输入 0–100%）"
                    onClick={() => {
                      setJumpValue(String(bookPercent));
                      setJumpOpen(true);
                    }}
                  >
                    · 全书 {bookPercent}%
                  </button>
                )}
              </span>
              <button
                type="button"
                className="btn ghost"
                onClick={() => (isPdf ? pdfTurn(1) : goChapter(1, 0))}
                disabled={nextDisabled}
              >
                {isPdf ? "下一页" : "下一章"}
              </button>
            </div>
          </>
        )}
      </header>

      {fontOpen && fonts && !isPdf && (
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

      {jumpOpen && (
        <div
          className="jump-pop-wrap"
          onClick={() => setJumpOpen(false)}
        >
          <div
            className="jump-pop"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              if (e.key === "Enter") confirmJump();
              if (e.key === "Escape") setJumpOpen(false);
            }}
          >
            <span className="jump-label">跳到全书位置</span>
            <input
              autoFocus
              inputMode="decimal"
              value={jumpValue}
              onChange={(e) => setJumpValue(e.target.value)}
              placeholder="0–100"
            />
            <span className="jump-unit">%</span>
            <button
              type="button"
              className="btn small paint"
              onClick={confirmJump}
            >
              跳转
            </button>
            <button
              type="button"
              className="btn small ghost"
              onClick={() => setJumpOpen(false)}
            >
              取消
            </button>
          </div>
        </div>
      )}

      {error && <div className="banner">{error}</div>}
      {book && fonts && !isPdf && !fonts.useOriginalFonts && !fonts.customFontsActive && (
        <div className="banner">自定义字体未齐，当前仍按原书 CSS。</div>
      )}
      {/* 打开时的格式提示（PDF 正文用了未嵌入的非标准字体 → 可能缺字）。浅色
          纸条 + 可关闭，不抢正文。 */}
      {book && book.warnings.length > 0 && !warnDismissed && (
        <div className="banner notice" role="status">
          <span>{book.warnings.join("；")}</span>
          <button
            type="button"
            className="btn ghost small"
            onClick={() => setWarnDismissed(true)}
          >
            关闭
          </button>
        </div>
      )}
      {metaEntry && (
        <BookMetaPanel
          entry={metaEntry}
          onClose={() => setMetaEntry(null)}
          onSaved={() => {
            setMetaEntry(null);
            void loadLibrary();
          }}
        />
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
              spineFallback={!isPdf}
            />
          </>
        )}
        {highlightsOpen && book && !isPdf && (
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
              notesById={notesById}
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
              onImport={() => void openBook()}
              onDelete={(entry) => void deleteBook(entry)}
              onEditMeta={(entry) => setMetaEntry(entry)}
            />
          )}
          {/* PDF：独立的连续纸带视图（SumatraPDF 式），不走 iframe / 分栏分页器，
              也不走 get_chapter（图片 URL 由 PdfView 直接拼）。 */}
          {book && isPdf && (
            <div className="page">
              <PdfView
                key={book.id}
                ref={pdfRef}
                bookId={book.id}
                resourceOrigin={resourceOrigin}
                pageCount={spine.length}
                pageSizes={book.pageSizes}
                fit={pdfFit}
                spread={pdfFit === "page" ? pdfSpread : "single"}
                initialPage={index + 1}
                onState={onPdfState}
              />
            </div>
          )}
          {/* EPUB：一字未变的分栏分页器路径。 */}
          {book && !isPdf && chapterHtml && (
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
                notesById={notesById}
                onSaveNote={saveNote}
                bookPos={bookPos}
                pendingHighlight={pendingHighlight}
                onHighlightLocated={onHighlightLocated}
                pendingFragment={pendingFragment}
                onFragmentLocated={() => setPendingFragment(null)}
                onProgress={queueProgress}
                onUsedFonts={setUsedFonts}
                onPageInfo={setPageInfo}
                onNeedChapter={(delta) =>
                  // 章边界续翻：向回翻要停在上一章末尾。
                  stepChapter(delta)
                }
                onFollowBookHref={goToHref}
              />
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
