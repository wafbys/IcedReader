import { chapterIndex, type HighlightRecord, type SpineItem } from "./types";
import { HIGHLIGHT_COLORS } from "./highlights";

type Props = {
  highlights: HighlightRecord[];
  spine: SpineItem[];
  currentHref: string;
  /** 备注正文（notes.md 用户区）id → 文本；有备注的划线显示 ✎ 预览。 */
  notesById: Record<string, string>;
  onSelect: (rec: HighlightRecord) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
};

function chapterLabel(spine: SpineItem[], href: string): string {
  const idx = chapterIndex(spine, href);
  if (idx < 0) return href;
  return spine[idx].title?.trim() || spine[idx].href;
}

function fmtTime(secs: number): string {
  const d = new Date(secs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(
    d.getHours(),
  )}:${p(d.getMinutes())}`;
}

export default function HighlightsPanel({
  highlights,
  spine,
  currentHref,
  notesById,
  onSelect,
  onDelete,
  onClose,
}: Props) {
  const currentIdx = chapterIndex(spine, currentHref);

  return (
    <aside className="toc-drawer" aria-label="划线">
      <div className="toc-head">
        <strong>划线</strong>
        <button type="button" className="btn ghost small" onClick={onClose}>
          关闭
        </button>
      </div>
      {highlights.length === 0 ? (
        <p className="toc-empty">
          还没有划线。阅读时选中文字即可划线；点已划线可写备注。
        </p>
      ) : (
        <nav className="toc-nav hl-nav">
          <ul className="toc-list">
            {highlights.map((rec) => {
              const idx = chapterIndex(spine, rec.href);
              const current = idx >= 0 && idx === currentIdx;
              const color = HIGHLIGHT_COLORS[rec.color] ?? HIGHLIGHT_COLORS.yellow;
              const note = notesById[rec.id];
              return (
                <li key={rec.id} className="hl-item">
                  <button
                    type="button"
                    className={`hl-main${current ? " current" : ""}`}
                    onClick={() => onSelect(rec)}
                  >
                    <span className="hl-head">
                      <span
                        className="hl-swatch"
                        style={{ background: color.bg }}
                        title={`${color.label}划线`}
                      />
                      <span className="hl-tag">{color.label}</span>
                      <span className="hl-time">划于 {fmtTime(rec.createdAt)}</span>
                    </span>
                    <span className="hl-text">{rec.text}</span>
                    {note !== undefined && note !== "" && (
                      <span className="hl-note" title="备注（完整内容见 notes.md）">
                        ✎ {note}
                      </span>
                    )}
                    <span className="hl-meta">{chapterLabel(spine, rec.href)}</span>
                  </button>
                  <button
                    type="button"
                    className="hl-del"
                    title={note ? "删除划线（notes.md 保留内容并记删除时间）" : "删除划线"}
                    aria-label="删除划线"
                    onClick={() => onDelete(rec.id)}
                  >
                    ✕
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>
      )}
    </aside>
  );
}
