import {
  FONT_SLOTS,
  slotLabel,
  type FontSettings,
  type FontSlotId,
  type PublisherFontReport,
  type UsedFontReport,
} from "./types";

type Props = {
  settings: FontSettings;
  publisherFonts: PublisherFontReport | null;
  usedFonts: UsedFontReport | null;
  busy: boolean;
  onToggleOriginal: (useOriginal: boolean) => void;
  onUpload: (slot: FontSlotId) => void;
  onClear: (slot: FontSlotId) => void;
};

export default function FontPanel({
  settings,
  publisherFonts,
  usedFonts,
  busy,
  onToggleOriginal,
  onUpload,
  onClear,
}: Props) {
  const missing = settings.missingSlots;
  const showWait =
    !settings.useOriginalFonts && !settings.customFontsActive;

  return (
    <div className="font-bar">
      <div className="font-bar-row">
        <label className="font-check">
          <input
            type="checkbox"
            checked={settings.useOriginalFonts}
            disabled={busy}
            onChange={(e) => onToggleOriginal(e.target.checked)}
          />
          使用原书字体
        </label>
        {settings.customFontsActive && (
          <span className="font-ok">正在使用自定义字体</span>
        )}
      </div>
      <UsedSpec report={usedFonts} />
      <PublisherSpec report={publisherFonts} />
      {showWait && (
        <p className="font-warn">
          还缺：{missing.map(slotLabel).join("、")}。当前仍按原书 CSS。
        </p>
      )}
      {FONT_SLOTS.map((slot) => {
        const file = settings.fonts[slot.id];
        const ready = !missing.includes(slot.id);
        return (
          <div className="font-slot" key={slot.id}>
            <span className="font-slot-name">{slot.label}</span>
            <span className="font-file" title={file?.originalName ?? ""}>
              {file
                ? ready
                  ? file.originalName
                  : `${file.originalName}（文件无效或缺失）`
                : "未上传"}
            </span>
            <button
              type="button"
              className="btn ghost small"
              disabled={busy}
              onClick={() => onUpload(slot.id)}
            >
              {file ? "更换" : "上传"}
            </button>
            <button
              type="button"
              className="btn ghost small"
              disabled={busy || !file}
              onClick={() => onClear(slot.id)}
            >
              清除
            </button>
          </div>
        );
      })}
    </div>
  );
}

function usedLabel(source: UsedFontReport["fonts"][number]["source"]): string {
  if (source === "specified") return "原书指定";
  if (source === "generic") return "泛型";
  return "回退";
}

function UsedSpec({ report }: { report: UsedFontReport | null }) {
  return (
    <div className="font-spec">
      <div className="font-spec-title">本章实际渲染</div>
      <p className="font-spec-note">当前页绘制字形用的字体，不是 CSS 写法。悬停字体名可看该字体首次出现的文字。</p>
      {!report ? (
        <p className="font-spec-empty">打开一章并排版完成后显示。</p>
      ) : report.fonts.length === 0 ? (
        <p className="font-spec-empty">本章没有可统计的文字。</p>
      ) : (
        <ul className="font-spec-list font-spec-used">
          {report.fonts.map((row) => (
            <li key={row.family}>
              <code
                className="font-spec-val"
                title={row.sample ? `首次：${row.sample}` : undefined}
              >
                {row.family}
              </code>
              <span className="font-spec-src">
                {row.glyphCount} 字 · {usedLabel(row.source)}
              </span>
            </li>
          ))}
        </ul>
      )}
      {report?.error && <p className="font-warn">{report.error}</p>}
      {report && report.missingSpecified.length > 0 && (
        <p className="font-spec-empty">
          指定未安装：{report.missingSpecified.join("、")}
        </p>
      )}
    </div>
  );
}

function PublisherSpec({ report }: { report: PublisherFontReport | null }) {
  if (!report) {
    return (
      <div className="font-spec">
        <div className="font-spec-title">本章原书指定</div>
        <p className="font-spec-empty">打开一章后显示原书 CSS 原文（含 serif 等泛型）。</p>
      </div>
    );
  }
  const empty = report.declarations.length === 0 && report.faces.length === 0;
  return (
    <div className="font-spec">
      <div className="font-spec-title">本章原书指定</div>
      <p className="font-spec-note">按 CSS 原文列出，不是系统最终选用的文件。</p>
      {empty ? (
        <p className="font-spec-empty">
          本章未写 font-family。未指定，由引擎按泛型（serif 等）或系统默认决定。
        </p>
      ) : (
        <ul className="font-spec-list">
          {report.declarations.map((row, i) => (
            <li key={`${row.selector}|${row.value}|${row.source}|${i}`}>
              <code className="font-spec-sel" title={row.source}>
                {row.selector}
              </code>
              <code className="font-spec-val">{row.value}</code>
              <span className="font-spec-src">{row.source}</span>
            </li>
          ))}
        </ul>
      )}
      {report.faces.length > 0 && (
        <p className="font-spec-faces">
          书中 @font-face：{report.faces.join("、")}
        </p>
      )}
      {report.truncated && (
        <p className="font-spec-note">声明较多，已截断，未全部列出。</p>
      )}
    </div>
  );
}
