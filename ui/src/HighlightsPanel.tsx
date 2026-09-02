import { chapterIndex, type HighlightRecord, type SpineItem } from "./types";

type Props = {
  highlights: HighlightRecord[];
  spine: SpineItem[];
  currentHref: string;
  onSelect: (rec: HighlightRecord) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
};

function chapterLabel(spine: SpineItem[], href: string): string {
  const idx = chapterIndex(spine, href);
  if (idx < 0) return href;
  return spine[idx].title?.trim() || spine[idx].href;
}

export default function HighlightsPanel({
  highlights,
  spine,
  currentHref,
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
        <p className="toc-empty">还没有划线。阅读时选中文字即可划线。</p>
      ) : (
        <nav className="toc-nav hl-nav">
          <ul className="toc-list">
            {highlights.map((rec) => {
              const idx = chapterIndex(spine, rec.href);
              const current = idx >= 0 && idx === currentIdx;
              return (
                <li key={rec.id} className="hl-item">
                  <button
                    type="button"
                    className={`hl-main${current ? " current" : ""}`}
                    onClick={() => onSelect(rec)}
                  >
                    <span className="hl-text">{rec.text}</span>
                    <span className="hl-meta">{chapterLabel(spine, rec.href)}</span>
                  </button>
                  <button
                    type="button"
                    className="hl-del"
                    title="删除划线"
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
