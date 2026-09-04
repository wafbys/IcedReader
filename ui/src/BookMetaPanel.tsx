import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { BookMetaFields, BookMetaView, LibraryEntry } from "./types";

/** 非空字段用 " _ " 连接 —— 与 core::book_meta::join_title 同规则（仅预览用，
 *  裁决永远发生在 Rust 侧：md displayTitle 空 → 保存后由字段拼接接管）。 */
export function joinPreview(
  title: string,
  subtitle: string,
  volume: string,
): string {
  return [title, subtitle, volume]
    .map((p) => p.trim())
    .filter((p) => p.length > 0)
    .join(" _ ");
}

type Props = {
  entry: LibraryEntry;
  onClose: () => void;
  /** 保存成功后才调用（App 负责刷新书架并关闭面板）。 */
  onSaved: () => void;
};

export default function BookMetaPanel({ entry, onClose, onSaved }: Props) {
  const [view, setView] = useState<BookMetaView | null>(null);
  const [error, setError] = useState("");
  const [title, setTitle] = useState("");
  const [subtitle, setSubtitle] = useState("");
  const [volume, setVolume] = useState("");
  /** 手改框：初值 = md 里用户确认过的 displayTitle；空 = 派生模式。 */
  const [display, setDisplay] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setView(null);
    setError("");
    setSaving(false);
    invoke<BookMetaView>("get_book_meta", { fileName: entry.fileName })
      .then((v) => {
        if (cancelled) return;
        setView(v);
        setTitle(v.title);
        setSubtitle(v.subtitle);
        setVolume(v.volume);
        setDisplay(v.confirmedTitle);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [entry.fileName]);

  useEffect(() => {
    if (saving) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, saving]);

  const joined = view ? joinPreview(title, subtitle, volume) : "";
  // 保存后实际生效的名字：手改名 → 字段拼接 → 打开时的裁决结果（dc:title/文件名）。
  const effective =
    display.trim() || joined || view?.displayTitle.trim() || entry.title;
  // 手改框非空 = 用户确认过：自动填充不覆盖（按钮禁用）；没内容可填时也禁用。
  const canAutoFill = display.trim() === "" && joined !== "";

  const save = async () => {
    if (!view || saving) return;
    setSaving(true);
    setError("");
    try {
      const fields: BookMetaFields = { title, subtitle, volume, displayTitle: display };
      await invoke("set_book_meta", { fileName: entry.fileName, fields });
      onSaved();
    } catch (err) {
      setError(String(err));
      setSaving(false);
    }
  };

  return (
    <div
      className="meta-overlay"
      role="presentation"
      onPointerDown={(e) => {
        // 点遮罩（不含卡片）关闭；保存中不响应，避免误关。
        if (e.target === e.currentTarget && !saving) onClose();
      }}
    >
      <div
        className="meta-modal"
        role="dialog"
        aria-modal="true"
        aria-label="编辑书籍信息"
      >
        <header className="meta-head">
          <div className="meta-head-text">
            <strong>编辑书籍信息</strong>
            <span className="meta-file" title={entry.fileName}>
              {entry.fileName}
            </span>
          </div>
          <button
            type="button"
            className="btn ghost small"
            onClick={onClose}
            disabled={saving}
          >
            关闭
          </button>
        </header>

        {!view && !error && <p className="meta-note">读取中…</p>}
        {error && !view && <p className="meta-error">{error}</p>}

        {view && (
          <form
            className="meta-form"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <div className="meta-field">
              <span className="meta-cap">原书名（只读）</span>
              <div className="meta-ro" title={view.originalTitle}>
                {view.originalTitle || "（无书名，回退到文件名）"}
              </div>
              <p className="meta-note">
                首次导入这本书时程序见到的书名。只读保留原始信息，不随本次编辑改变。
              </p>
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-title">主书名</label>
              <input
                id="bookmeta-title"
                className="meta-input"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder={view.displayTitle || "主书名"}
                autoFocus
              />
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-subtitle">副标题</label>
              <input
                id="bookmeta-subtitle"
                className="meta-input"
                value={subtitle}
                onChange={(e) => setSubtitle(e.target.value)}
              />
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-volume">卷册</label>
              <input
                id="bookmeta-volume"
                className="meta-input"
                value={volume}
                onChange={(e) => setVolume(e.target.value)}
              />
            </div>

            <div className="meta-preview">
              <span className="meta-cap">拼接预览</span>
              <code className="meta-joined">{joined || "（字段留空时不拼接）"}</code>
              <p className="meta-note">
                非空字段按 空格 _ 空格 连接，空字段自动跳过；分隔符由程序生成
                （只出半角），不改动原书标题里自带的字符。
              </p>
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-display">显示名（可留空）</label>
              <div className="meta-row">
                <input
                  id="bookmeta-display"
                  className="meta-input"
                  value={display}
                  onChange={(e) => setDisplay(e.target.value)}
                  placeholder={effective}
                />
                <button
                  type="button"
                  className="btn ghost small"
                  onClick={() => setDisplay(joined)}
                  disabled={!canAutoFill}
                  title={
                    display.trim()
                      ? "显示名已手填：自动填充不覆盖手改，清空后可重新使用"
                      : joined
                        ? "把上方字段的拼接填入显示名"
                        : "字段都为空，没有可填充的内容"
                  }
                >
                  自动填充
                </button>
              </div>
              <p className="meta-note">
                留空 = 书架/阅读自动用上方字段拼接的标题（以后改字段即跟随）。
                填写 = 固定为该名字，字段改动和自动填充都不覆盖它；清空可回到自动拼接。
              </p>
            </div>

            <p className="meta-effect">
              保存后书架将显示：
              <strong title={effective}>{effective}</strong>
            </p>

            {error && <p className="meta-error">{error}</p>}

            <div className="meta-actions">
              <button type="submit" className="btn" disabled={saving || !view}>
                {saving ? "保存中…" : "保存"}
              </button>
              <button
                type="button"
                className="btn ghost"
                onClick={onClose}
                disabled={saving}
              >
                取消
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
