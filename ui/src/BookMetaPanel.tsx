import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { BookMetaFields, BookMetaView, LibraryEntry } from "./types";

/**
 * 拼接预览 —— 镜像 core::book_meta::join_title（裁决永远在 Rust 侧发生）：
 * `书名 [ _ 副标题] [ - 卷册] [ - 作者] [ - 译者] [ - 出版年份]
 * [ - 出版社] [ - ISBN…]`。` _ ` 只出现在书名与副标题之间，其后一律
 * ` - `；空段整体跳过；书名必填（为空则不拼接）；ISBN 未自带前缀时补
 * ASCII「ISBN 」、译者未自带「译者」时补「译者 」标签。
 */
export function joinPreview(f: {
  title: string;
  subtitle: string;
  volume: string;
  author: string;
  translator: string;
  year: string;
  publisher: string;
  isbn: string;
}): string {
  const title = f.title.trim();
  if (!title) return "";
  let head = title;
  const subtitle = f.subtitle.trim();
  if (subtitle) head += " _ " + subtitle;
  const parts = [head];
  for (const v of [f.volume.trim(), f.author.trim()]) {
    if (v) parts.push(v);
  }
  const translator = f.translator.trim();
  if (translator) parts.push(/^译者/.test(translator) ? translator : `译者 ${translator}`);
  for (const v of [f.year.trim(), f.publisher.trim()]) {
    if (v) parts.push(v);
  }
  const isbn = f.isbn.trim();
  if (isbn) parts.push(/^isbn/i.test(isbn) ? isbn : `ISBN ${isbn}`);
  return parts.join(" - ");
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
  const [author, setAuthor] = useState("");
  const [translator, setTranslator] = useState("");
  const [year, setYear] = useState("");
  const [publisher, setPublisher] = useState("");
  const [isbn, setIsbn] = useState("");
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
        setAuthor(v.author);
        setTranslator(v.translator);
        setYear(v.year);
        setPublisher(v.publisher);
        setIsbn(v.isbn);
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

  const fields = {
    title,
    subtitle,
    volume,
    author,
    translator,
    year,
    publisher,
    isbn,
  };
  const joined = view ? joinPreview(fields) : "";
  // 保存后实际生效的名字：手改名 → 字段拼接 → 打开时的裁决结果（dc:title/文件名）。
  const effective =
    display.trim() || joined || view?.displayTitle.trim() || entry.title;
  // 手改框非空 = 用户确认过：自动填充不覆盖（按钮禁用）；没内容可填时也禁用。
  const canAutoFill = display.trim() === "" && joined !== "";
  // 书名必填：留空时不能保存（标题只能回退原书名，走不了拼接）。
  const titleMissing = view !== null && title.trim() === "";

  const save = async () => {
    if (!view || saving || titleMissing) return;
    setSaving(true);
    setError("");
    try {
      const payload: BookMetaFields = { ...fields, displayTitle: display };
      await invoke("set_book_meta", { fileName: entry.fileName, fields: payload });
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
            id="bookmeta-form"
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
              <label htmlFor="bookmeta-title">
                主书名 <span className="meta-req">必填</span>
              </label>
              <input
                id="bookmeta-title"
                className="meta-input"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder={view.displayTitle || "主书名"}
                autoFocus
              />
              {titleMissing && (
                <p className="meta-error">主书名必填：留空时标题只能回退原书名，无法拼接。</p>
              )}
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
                placeholder="如 第二部、上"
              />
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-author">作者</label>
              <input
                id="bookmeta-author"
                className="meta-input"
                value={author}
                onChange={(e) => setAuthor(e.target.value)}
                placeholder="预填原书作者，可改可清空"
              />
              <p className="meta-note">已预填原书作者（多名用、连接）；清空则不拼入标题。</p>
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-translator">译者</label>
              <input
                id="bookmeta-translator"
                className="meta-input"
                value={translator}
                onChange={(e) => setTranslator(e.target.value)}
                placeholder="如 阳曦"
              />
              <p className="meta-note">
                填姓名即可；拼入标题时自动补「译者 」标签（已写「译者」开头则保留原样），留空不拼入。
              </p>
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-year">出版年份</label>
              <input
                id="bookmeta-year"
                className="meta-input"
                value={year}
                onChange={(e) => setYear(e.target.value)}
                placeholder="如 2008"
              />
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-publisher">出版社</label>
              <input
                id="bookmeta-publisher"
                className="meta-input"
                value={publisher}
                onChange={(e) => setPublisher(e.target.value)}
              />
            </div>

            <div className="meta-field">
              <label htmlFor="bookmeta-isbn">ISBN</label>
              <input
                id="bookmeta-isbn"
                className="meta-input"
                value={isbn}
                onChange={(e) => setIsbn(e.target.value)}
                placeholder="如 978-7-5366-9293-0"
              />
              <p className="meta-note">
                填号码即可；拼入标题时自动补 ASCII「ISBN 」前缀（你已写 ISBN 开头则保留原样）。
              </p>
            </div>

            <div className="meta-preview">
              <span className="meta-cap">拼接预览</span>
              <code className="meta-joined" title={joined || undefined}>
                {joined || "（书名必填；留空则不拼接）"}
              </code>
            </div>
            <p className="meta-note">
              书名 _ 副标题 - 卷册 - 作者 - 译者 - 出版年份 - 出版社 - ISBN。
              书名与副标题之间用 空格 _ 空格，其后各项用 空格 - 空格；
              空字段自动跳过，不会出现连续分隔符。符号由程序生成（只出半角）。
            </p>

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
                        : "主书名或字段都为空，没有可填充的内容"
                  }
                >
                  自动填充
                </button>
              </div>
              <p className="meta-note">
                留空 = 书架/阅读自动按上方模板拼接（以后改字段即跟随）。
                填写 = 固定为该名字，字段改动和自动填充都不覆盖它；清空可回到自动拼接。
              </p>
            </div>

            <p className="meta-effect">
              保存后书架将显示：
              <strong title={effective}>{effective}</strong>
              <span className="meta-note">
                （数据目录里的 epub 与同名 md 会按此名改名；若已有同名文件自动加
                -2、-3…，进度与划线一并保留。）
              </span>
            </p>
          </form>
        )}

        {view && (
          <div className="meta-actions">
            {error && <p className="meta-error meta-actions-error">{error}</p>}
            <button
              type="submit"
              form="bookmeta-form"
              className="btn"
              disabled={saving || titleMissing}
              title={titleMissing ? "主书名必填" : undefined}
            >
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
        )}
      </div>
    </div>
  );
}
