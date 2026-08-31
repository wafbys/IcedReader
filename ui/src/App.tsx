import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import ChapterFrame from "./ChapterFrame";
import { chapterIndex, type OpenedBook } from "./types";

export default function App() {
  const [book, setBook] = useState<OpenedBook | null>(null);
  const [index, setIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [chapterHtml, setChapterHtml] = useState("");
  const [restoreFraction, setRestoreFraction] = useState(0);

  const bookRef = useRef(book);
  bookRef.current = book;
  const indexRef = useRef(index);
  indexRef.current = index;
  const pending = useRef<{ key: string; href: string; fraction: number } | null>(
    null,
  );
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
    const onUnload = () => flushProgress();
    window.addEventListener("beforeunload", onUnload);
    return () => {
      window.removeEventListener("beforeunload", onUnload);
      flushProgress();
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
      return;
    }
    let cancelled = false;
    invoke<string>("get_chapter", { id: book.id, href: current.href })
      .then((html) => {
        if (!cancelled) setChapterHtml(html);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [book, current]);

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
        setRestoreFraction(
          restored >= 0 && opened.progress ? opened.progress.fraction : 0,
        );
        setChapterHtml("");
      } catch (err) {
        setError(String(err));
        setBook(null);
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

  const go = (delta: number) => {
    flushProgress();
    setRestoreFraction(0);
    setIndex((i) => Math.min(spine.length - 1, Math.max(0, i + delta)));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight") go(1);
      if (e.key === "ArrowLeft") go(-1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="shell">
      <header className="chrome">
        <div className="brand">IcedReader</div>
        <button type="button" className="btn" onClick={openEpub} disabled={busy}>
          {busy ? "打开中…" : "打开 EPUB"}
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
                onClick={() => go(-1)}
                disabled={index <= 0}
              >
                上一章
              </button>
              <span className="pos">
                {spine.length ? index + 1 : 0} / {spine.length}
              </span>
              <button
                type="button"
                className="btn ghost"
                onClick={() => go(1)}
                disabled={index >= spine.length - 1}
              >
                下一章
              </button>
            </div>
          </>
        )}
      </header>

      {error && <div className="banner">{error}</div>}

      <main className="stage">
        {!book && (
          <div className="empty">
            <p>打开一本 EPUB，开始阅读。</p>
            <p className="hint">进度按章节和章内比例保存，换设备、改字号也能对上。</p>
          </div>
        )}
        {book && chapterHtml && (
          <ChapterFrame
            html={chapterHtml}
            restoreFraction={restoreFraction}
            onProgress={queueProgress}
          />
        )}
      </main>
    </div>
  );
}
