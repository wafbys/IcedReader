import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { fmtBytes } from "./format";
import { useModalDialog } from "./modal";
import type {
  BookComparison,
  CompareAxis,
  CompareAxisGroup,
  CompareVerdict,
  LibraryEntry,
} from "./types";

/**
 * 同书对照面板。定位见 docs/ideas/book-compare.md：
 * 每本书的 优/良/中 与依据是**单本独立**的；这里只在书架已经判定「同书」时
 * 叠加一层对照，回答角标回答不了的那个问题——**留哪一本**。
 *
 * 三条呈现约定：
 * - 默认只显示有差异的组，整组打平就折成一行；「显示全部」可展开。
 * - 只呈现的轴（正文字数、封面体积）也画出来，但标「不计优劣」。
 * - 面板里不删书；删除仍走书架三点菜单的确认流程。
 */

const GROUP_ORDER: CompareAxisGroup[] = [
  "content",
  "apparatus",
  "packaging",
  "provenance",
];

const GROUP_LABEL: Record<CompareAxisGroup, string> = {
  content: "正文内容",
  apparatus: "书内装置",
  packaging: "打包",
  provenance: "溯源",
};

const VERDICT_LABEL: Record<CompareVerdict, string> = {
  tie: "平",
  winner: "有差",
  shared: "并列",
  incomparable: "不可比",
  presented: "不计优劣",
};

/** 一根轴是否值得单独列出来。 */
function isDifferent(axis: CompareAxis): boolean {
  return (
    axis.verdict === "winner" ||
    axis.verdict === "shared" ||
    axis.verdict === "incomparable"
  );
}

/**
 * 列标题。两本同书常常**书名一样**（真正的区别在文件名里，比如体积前缀），
 * 所以同名时把文件名补上——Rust 侧只给下标，就是为此。
 */
function columnLabel(
  index: number,
  columns: BookComparison["columns"],
): string {
  const c = columns[index];
  const repeated = columns.filter((x) => x.title === c.title).length > 1;
  return repeated ? `${c.title}（${c.fileName}）` : c.title;
}

type Props = {
  entry: LibraryEntry;
  entries: LibraryEntry[];
  onClose: () => void;
  onOpen: (entry: LibraryEntry) => void;
};

export default function ComparePanel({ entry, entries, onClose, onOpen }: Props) {
  const [data, setData] = useState<BookComparison | null>(null);
  const [error, setError] = useState("");
  const [showAll, setShowAll] = useState(false);

  // 同书那一组：自己 + 书架指认的同书。书架上没有的（已被改名/删除）忽略。
  const group = useMemo(() => {
    const byName = new Map(entries.map((e) => [e.fileName, e]));
    const list: LibraryEntry[] = [entry];
    for (const name of entry.duplicates) {
      const peer = byName.get(name);
      if (peer) list.push(peer);
    }
    return list;
  }, [entry, entries]);

  useEffect(() => {
    let cancelled = false;
    setData(null);
    setError("");
    invoke<BookComparison>("compare_books", {
      fileNames: group.map((e) => e.fileName),
    })
      .then((v) => {
        if (!cancelled) setData(v);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [group]);

  const close = () => onClose();
  // 真 modal：原生 <dialog> + showModal（背景 inert、焦点圈定、Esc 关闭；点遮罩不关）。
  const dialogRef = useModalDialog(close);

  return (
    <dialog ref={dialogRef} className="cmp-modal" aria-label="同书对照">
      <header className="cmp-head">
        <div className="cmp-head-text">
          <strong>同书对照</strong>
          <span className="cmp-file">
            {group.map((e) => e.fileName).join("  ×  ")}
          </span>
        </div>
        <button type="button" className="btn ghost small" onClick={close}>
          关闭
        </button>
      </header>

      {!data && !error && (
        <p className="cmp-note cmp-busy">
          正在对照…（某本还没分析过时，要现场读一遍整本书，大书可能要几秒）
        </p>
      )}
      {error && <p className="cmp-error">{error}</p>}

      {data && (
        // --cmp-cols lives on the shared ancestor: `.cmp-cols` and `.cmp-row`
        // are siblings, so a variable set on one never reaches the other and
        // every row would silently fall back to two columns.
        <div
          className="cmp-body"
          style={{ ["--cmp-cols" as string]: data.columns.length }}
        >
          <div className="cmp-verdict">
            {data.lean != null ? (
              <p className="cmp-lean">
                倾向保留
                <strong>{columnLabel(data.lean, data.columns)}</strong>
                {/* 体积常常正是「为什么留这本」的背景数字（同名两版往往只差在文件大小），
                    所以点名的那一本旁边直接给出大小，不用去表头里找。 */}
                <span
                  className="cmp-lean-size"
                  title={`书籍文件大小：${data.columns[data.lean].sizeBytes.toLocaleString()} 字节`}
                >
                  {fmtBytes(data.columns[data.lean].sizeBytes)}
                </span>
              </p>
            ) : (
              <p className="cmp-lean cmp-lean-none">没有单一赢家</p>
            )}
            {data.lean != null && data.leanReason.length > 0 && (
              <ul className="cmp-reasons">
                {data.leanReason.map((r, i) => (
                  <li key={i}>{r}</li>
                ))}
              </ul>
            )}
            {data.conclusion.map((line, i) => (
              <p className="cmp-note" key={i}>
                {line}
              </p>
            ))}
          </div>

          <div className="cmp-cols">
            <div className="cmp-col-head cmp-col-axis">项目</div>
            {data.columns.map((c, i) => (
              <div className="cmp-col-head" key={c.fileName}>
                <strong title={c.title}>{columnLabel(i, data.columns)}</strong>
                <span className="cmp-col-size">{fmtBytes(c.sizeBytes)}</span>
                {c.quality && (
                  <span className={`cmp-col-grade g-${c.quality}`}>{c.quality}</span>
                )}
                <button
                  type="button"
                  className="btn ghost small cmp-open"
                  onClick={() => {
                    const target = group[i];
                    if (target) {
                      close();
                      onOpen(target);
                    }
                  }}
                >
                  打开这本
                </button>
              </div>
            ))}
          </div>

          <div className="cmp-rows">
            {GROUP_ORDER.map((groupKey) => {
              const axes = data.axes.filter((a) => a.group === groupKey);
              if (axes.length === 0) return null;
              const differing = axes.filter(isDifferent);
              if (differing.length === 0 && !showAll) {
                return (
                  <p className="cmp-tied-group" key={groupKey}>
                    {GROUP_LABEL[groupKey]}：{axes.length} 项一致
                  </p>
                );
              }
              return (
                <div className="cmp-group" key={groupKey}>
                  <h4 className="cmp-group-title">{GROUP_LABEL[groupKey]}</h4>
                  {axes.map((axis) => (
                    <div
                      className="cmp-row"
                      key={axis.key}
                      title={`${axis.label}：${VERDICT_LABEL[axis.verdict]}${
                        axis.note ? `\n${axis.note}` : ""
                      }`}
                    >
                      <div className="cmp-axis" title={axis.note ?? undefined}>
                        <span className="cmp-axis-label">{axis.label}</span>
                        {axis.verdict === "presented" && (
                          <span className="cmp-axis-tag">不计优劣</span>
                        )}
                        {axis.verdict === "incomparable" && (
                          <span className="cmp-axis-tag">比不了</span>
                        )}
                      </div>
                      {axis.cells.map((cell, i) => (
                        <div
                          className={`cmp-cell mark-${cell.mark}`}
                          key={`${axis.key}-${i}`}
                        >
                          {cell.display}
                        </div>
                      ))}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>

          {data.chapterDiff && (
            <div className="cmp-chapters">
              <h4 className="cmp-group-title">
                逐章对照（{data.chapterDiff.labels.length} 个单元，
                {data.chapterDiff.identical.filter((x) => !x).length} 个正文不同）
              </h4>
              <div className="cmp-strip" role="img" aria-label="逐章差异条带">
                {data.chapterDiff.identical.map((same, i) => (
                  <span
                    key={i}
                    className={same ? "cmp-bar same" : "cmp-bar diff"}
                    title={
                      (data.chapterDiff?.labels[i] || `第 ${i + 1} 单元`) +
                      (same
                        ? "：正文一致"
                        : `：正文不同（${data.chapterDiff?.deltas[i] ?? 0} 字）`)
                    }
                  />
                ))}
              </div>
              <p className="cmp-note">
                绿 = 正文逐字一致（差异只在打包），红 = 正文真的不同。
              </p>
            </div>
          )}

          {data.axes.some(isDifferent) && (
            <label className="cmp-showall">
              <input
                type="checkbox"
                checked={showAll}
                onChange={(e) => setShowAll(e.target.checked)}
              />
              连一致的项一起显示
            </label>
          )}
        </div>
      )}
    </dialog>
  );
}
