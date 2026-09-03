import { useEffect, useState } from "react";
import type { LibraryEntry } from "./types";

type Props = {
  entries: LibraryEntry[];
  origin: string;
  busy: boolean;
  onOpen: (path: string) => void;
  onImport: () => void;
  onDelete: (entry: LibraryEntry) => void;
};

function progressLabel(entry: LibraryEntry): string {
  if (entry.openError) return "无法打开";
  const pct =
    entry.fraction != null
      ? ` · ${Math.round(Math.min(1, Math.max(0, entry.fraction)) * 100)}%`
      : "";
  if (entry.chapterIndex != null && entry.chapterCount != null) {
    const n = entry.chapterIndex + 1;
    const title = entry.chapterTitle ? ` · ${entry.chapterTitle}` : "";
    return `第 ${n}/${entry.chapterCount} 章${title}${pct}`;
  }
  if (entry.updatedAt != null || entry.fraction != null) {
    return `阅读中${pct}`;
  }
  return entry.chapterCount ? `共 ${entry.chapterCount} 章 · 未读` : "未读";
}

function coverUrl(origin: string, fileName: string, coverRev: string): string {
  const base = `${origin.replace(/\/$/, "")}/library-cover/${encodeURIComponent(fileName)}`;
  return coverRev ? `${base}?r=${encodeURIComponent(coverRev)}` : base;
}

function Cover({ entry, origin }: { entry: LibraryEntry; origin: string }) {
  const [broken, setBroken] = useState(false);
  const mark = (entry.title.trim().charAt(0) || "书").toUpperCase();
  if (!entry.hasCover || !origin || broken) {
    return (
      <div className="lib-cover lib-cover-fallback" aria-hidden>
        {mark}
      </div>
    );
  }
  return (
    <img
      className="lib-cover"
      src={coverUrl(origin, entry.fileName, entry.coverRev)}
      alt=""
      onError={() => setBroken(true)}
    />
  );
}

/** Vertical three-dot glyph (U+22EE is not reliably present in every font). */
function MoreGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
      <circle cx="8" cy="3.2" r="1.7" fill="currentColor" />
      <circle cx="8" cy="8" r="1.7" fill="currentColor" />
      <circle cx="8" cy="12.8" r="1.7" fill="currentColor" />
    </svg>
  );
}

export default function Library({
  entries,
  origin,
  busy,
  onOpen,
  onImport,
  onDelete,
}: Props) {
  /** Path of the entry whose menu is open (one at a time). */
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const closeMenu = () => setMenuFor(null);
  const toggleMenu = (path: string) =>
    setMenuFor((current) => (current === path ? null : path));

  useEffect(() => {
    if (!menuFor) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeMenu();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [menuFor]);

  if (entries.length === 0) {
    return (
      <div className="empty">
        <p>书库还是空的。</p>
        <p className="hint">打开一本 EPUB，会复制进程序目录下的 data/library/。</p>
        <button type="button" className="btn" onClick={onImport} disabled={busy}>
          {busy ? "打开中…" : "打开 EPUB"}
        </button>
      </div>
    );
  }

  return (
    <div
      className="lib"
      onPointerDown={(e) => {
        // A press anywhere outside the open menu closes it; card presses then
        // still fire their own click (opening a book only needs a second press).
        const target = e.target as Element;
        if (menuFor && !target.closest(".lib-menu, .lib-more")) closeMenu();
      }}
    >
      <ul className="lib-grid">
        {entries.map((entry) => (
          <li key={entry.path} className="lib-item">
            <div className="lib-cover-slot">
              <button
                type="button"
                className="lib-card lib-cover-btn"
                disabled={busy || !!entry.openError}
                title={entry.openError ?? entry.title}
                onClick={() => onOpen(entry.path)}
              >
                <Cover
                  key={entry.coverRev || entry.path}
                  entry={entry}
                  origin={origin}
                />
              </button>
              <div className="lib-more-anchor">
                <button
                  type="button"
                  className="lib-more"
                  aria-label={`更多操作：${entry.title}`}
                  aria-haspopup="menu"
                  aria-expanded={menuFor === entry.path}
                  disabled={busy}
                  onClick={() => toggleMenu(entry.path)}
                >
                  <MoreGlyph />
                </button>
                {menuFor === entry.path && (
                  <div className="lib-menu" role="menu">
                    {/* Future per-book commands get added here. */}
                    <button
                      type="button"
                      role="menuitem"
                      onClick={() => {
                        closeMenu();
                        onDelete(entry);
                      }}
                    >
                      从书库删除
                    </button>
                  </div>
                )}
              </div>
            </div>
            <button
              type="button"
              className="lib-info"
              disabled={busy || !!entry.openError}
              title={entry.openError ?? entry.title}
              onClick={() => onOpen(entry.path)}
            >
              <strong>{entry.title}</strong>
              <span>
                {entry.authors.length ? entry.authors.join("、") : "未知作者"}
              </span>
              <span className="lib-progress">{progressLabel(entry)}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
