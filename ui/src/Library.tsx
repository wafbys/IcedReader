import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { fmtBytes } from "./format";
import type { LibraryEntry } from "./types";

/** Hover tooltip content for one quality badge (优/良/中) with its reasons. */
type GradeTip = {
  grade: string;
  /** Measured facts and merits behind the grade. */
  plus: string[];
  /** What held the book back; shown under a 减分项 heading when non-empty. */
  minus: string[];
  /** Anchor (badge) viewport rect, for placing the viewport-fixed bubble. */
  anchorLeft: number;
  anchorTop: number;
  anchorBottom: number;
};

type Props = {
  entries: LibraryEntry[];
  origin: string;
  busy: boolean;
  onOpen: (path: string) => void;
  onImport: () => void;
  onDelete: (entry: LibraryEntry) => void;
  onEditMeta: (entry: LibraryEntry) => void;
  onCompare: (entry: LibraryEntry) => void;
};

/** 封面 / 书名悬停提示：书名 + 书籍文件大小（同一本书两处看到同一份信息）。 */
function bookTip(entry: LibraryEntry): string {
  return `${entry.title}\n大小 ${fmtBytes(entry.sizeBytes)}`;
}

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

/** 书架顶部统计：总数、未读（从未打开）、已打开（有进度记录）、坏书。 */
function ShelfStats({ entries }: { entries: LibraryEntry[] }) {
  if (entries.length === 0) return null;
  let unread = 0;
  let opened = 0;
  let broken = 0;
  for (const e of entries) {
    if (e.openError) {
      broken += 1;
    } else if (e.updatedAt != null || e.fraction != null) {
      opened += 1;
    } else {
      unread += 1;
    }
  }
  const parts = [`共 ${entries.length} 本`];
  if (unread > 0) parts.push(`未读 ${unread}`);
  if (opened > 0) parts.push(`已打开 ${opened}`);
  if (broken > 0) parts.push(`无法打开 ${broken}`);
  return <p className="lib-stats">{parts.join(" · ")}</p>;
}

function coverUrl(
  origin: string,
  fileName: string,
  coverRev: string,
  retry = 0,
): string {
  const base = `${origin.replace(/\/$/, "")}/library-cover/${encodeURIComponent(fileName)}`;
  const rev = coverRev ? `r=${encodeURIComponent(coverRev)}` : "";
  const suffix = [rev, retry > 0 ? `n=${retry}` : ""].filter(Boolean).join("&");
  return suffix ? `${base}?${suffix}` : base;
}

/** The protocol answers a cover it has not rendered yet with a 2×2 placeholder
 *  (rendering inside a request would freeze the shelf — a PDF cover costs up to
 *  seconds). Recognise it by its size and retry a couple of times while the
 *  background render finishes, instead of showing a blank cover forever. */
const PLACEHOLDER_MAX_PX = 2;
const COVER_RETRIES = 3;
const COVER_RETRY_MS = 700;

function Cover({ entry, origin }: { entry: LibraryEntry; origin: string }) {
  const [broken, setBroken] = useState(false);
  const [retry, setRetry] = useState(0);
  const mark = (entry.title.trim().charAt(0) || "书").toUpperCase();
  const timer = useRef<number | null>(null);
  useEffect(() => {
    return () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    };
  }, []);
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
      src={coverUrl(origin, entry.fileName, entry.coverRev, retry)}
      alt=""
      onError={() => setBroken(true)}
      onLoad={(event) => {
        const img = event.currentTarget;
        if (img.naturalWidth > PLACEHOLDER_MAX_PX || retry >= COVER_RETRIES) return;
        if (timer.current !== null) window.clearTimeout(timer.current);
        timer.current = window.setTimeout(
          () => setRetry((n) => n + 1),
          COVER_RETRY_MS,
        );
      }}
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
  onEditMeta,
  onCompare,
}: Props) {
  /** Path of the entry whose menu is open (one at a time). */
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const closeMenu = () => setMenuFor(null);

  /** Quality-badge tooltip (one at a time, viewport-fixed like .note-tip). */
  const [gradeTip, setGradeTip] = useState<GradeTip | null>(null);
  const gradeTipRef = useRef<HTMLDivElement | null>(null);
  const closeGradeTip = () => setGradeTip(null);

  const showGradeTip = (el: HTMLElement, entry: LibraryEntry) => {
    const r = el.getBoundingClientRect();
    setGradeTip({
      grade: entry.quality ?? "",
      plus: entry.qualityPlus,
      minus: entry.qualityMinus,
      anchorLeft: r.left,
      anchorTop: r.top,
      anchorBottom: r.bottom,
    });
  };

  // Place the bubble under the badge before paint; flip above / clamp when it
  // would run off the viewport edge.
  useLayoutEffect(() => {
    const el = gradeTipRef.current;
    if (!el || !gradeTip) return;
    const w = el.offsetWidth || 280;
    const h = el.offsetHeight || 90;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left = gradeTip.anchorLeft;
    let top = gradeTip.anchorBottom + 6;
    if (left + w + 8 > vw) left = Math.max(8, vw - w - 8);
    if (top + h + 8 > vh) {
      top = gradeTip.anchorTop - h - 6;
      if (top < 8) top = 8;
    }
    el.style.left = `${Math.round(left)}px`;
    el.style.top = `${Math.round(top)}px`;
  }, [gradeTip]);

  // Fixed positioning goes stale on window resize while the bubble is open.
  useEffect(() => {
    if (!gradeTip) return;
    const onResize = () => setGradeTip(null);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [gradeTip]);
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
        <p className="hint">打开一本 EPUB / PDF，会复制进程序目录下的 data/library/。</p>
        <button type="button" className="btn" onClick={onImport} disabled={busy}>
          {busy ? "打开中…" : "打开电子书"}
        </button>
      </div>
    );
  }

  return (
    <div
      className="lib"
      onScroll={closeGradeTip}
      onPointerDown={(e) => {
        // A press anywhere outside the open menu closes it; card presses then
        // still fire their own click (opening a book only needs a second press).
        const target = e.target as Element;
        if (menuFor && !target.closest(".lib-menu, .lib-more")) closeMenu();
      }}
    >
      {busy && (
        <div className="lib-busy" role="status">
          正在导入并分析排版与质量…
        </div>
      )}
      <ShelfStats entries={entries} />
      <ul className="lib-grid">
        {entries.map((entry) => (
          <li key={entry.path} className="lib-item">
            <div className="lib-cover-slot">
              {entry.quality && !entry.openError && (
                <span
                  className={`lib-grade g-${entry.quality}`}
                  onMouseEnter={(e) => showGradeTip(e.currentTarget, entry)}
                  onMouseLeave={closeGradeTip}
                  onClick={() => {
                    // The badge sits on the cover: keep a click there opening
                    // the book, like the pointer-events pass-through did before.
                    if (!busy) onOpen(entry.path);
                  }}
                >
                  {entry.quality}
                </span>
              )}
              <button
                type="button"
                className="lib-card lib-cover-btn"
                disabled={busy || !!entry.openError}
                title={entry.openError ?? bookTip(entry)}
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
                    <button
                      type="button"
                      role="menuitem"
                      onClick={() => {
                        closeMenu();
                        onEditMeta(entry);
                      }}
                    >
                      编辑元数据…
                    </button>
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
              title={entry.openError ?? bookTip(entry)}
              onClick={() => onOpen(entry.path)}
            >
              <strong>{entry.title}</strong>
              <span>
                {entry.authors.length ? entry.authors.join(", ") : "未知作者"}
              </span>
              <span className="lib-progress">{progressLabel(entry)}</span>
            </button>
            {/* 同书提示做成面板入口。它必须是 .lib-info 的**兄弟**而不是子节点：
                外面那个是 <button>，按钮里不能再嵌可点控件。 */}
            {entry.duplicates.length > 0 && (
              <button
                type="button"
                className="lib-dup"
                disabled={busy}
                title={`与以下书为同一本：\n${entry.duplicates.join("\n")}\n\n点开对照两版差别`}
                onClick={() => onCompare(entry)}
              >
                同书 · 另有 {entry.duplicates.length} 本
              </button>
            )}
          </li>
        ))}
      </ul>
      {gradeTip && (
        <div ref={gradeTipRef} className="lib-grade-tip" role="tooltip">
          <span className={`tip-grade g-${gradeTip.grade}`}>
            质量：{gradeTip.grade}
          </span>
          {gradeTip.plus.length > 0 && (
            <ul>
              {gradeTip.plus.map((reason, i) => (
                <li key={`p${i}`}>{reason}</li>
              ))}
            </ul>
          )}
          {gradeTip.minus.length > 0 && (
            <>
              <span className="tip-minus-label">减分项</span>
              <ul className="tip-minus">
                {gradeTip.minus.map((reason, i) => (
                  <li key={`m${i}`}>{reason}</li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
    </div>
  );
}
