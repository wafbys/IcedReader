import { useState } from "react";
import type { LibraryEntry } from "./types";

type Props = {
  entries: LibraryEntry[];
  origin: string;
  busy: boolean;
  onOpen: (path: string) => void;
  onImport: () => void;
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

export default function Library({
  entries,
  origin,
  busy,
  onOpen,
  onImport,
}: Props) {
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
    <div className="lib">
      <ul className="lib-grid">
        {entries.map((entry) => (
          <li key={entry.path}>
            <button
              type="button"
              className="lib-card"
              disabled={busy || !!entry.openError}
              title={entry.openError ?? entry.title}
              onClick={() => onOpen(entry.path)}
            >
              <Cover
                key={entry.coverRev || entry.path}
                entry={entry}
                origin={origin}
              />
              <div className="lib-info">
                <strong>{entry.title}</strong>
                <span>
                  {entry.authors.length ? entry.authors.join("、") : "未知作者"}
                </span>
                <span className="lib-progress">{progressLabel(entry)}</span>
              </div>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
