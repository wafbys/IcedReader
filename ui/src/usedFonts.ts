/** Actual fonts used to paint the current chapter, from the rendered iframe. */

const MISSING = "__IcedReaderMissingFont__";
const GENERIC = new Set([
  "serif",
  "sans-serif",
  "monospace",
  "cursive",
  "fantasy",
  "system-ui",
  "ui-serif",
  "ui-sans-serif",
  "ui-monospace",
  "ui-rounded",
  "emoji",
  "math",
  "fangsong",
]);

const CJK_FONTS = [
  "IcedReaderSerif",
  "IcedReaderSans",
  "IcedReaderMono",
  "Microsoft YaHei UI",
  "Microsoft YaHei",
  "微软雅黑",
  "Microsoft JhengHei",
  "微软正黑体",
  "SimSun",
  "NSimSun",
  "宋体",
  "SimHei",
  "黑体",
  "KaiTi",
  "楷体",
  "FangSong",
  "仿宋",
  "DengXian",
  "等线",
  "PMingLiU",
  "MingLiU",
  "Source Han Sans SC",
  "Source Han Serif SC",
  "Noto Sans CJK SC",
  "Noto Serif CJK SC",
  "PingFang SC",
  "STHeiti",
  "STSong",
  "STKaiti",
];

const LATIN_FONTS = [
  "Segoe UI",
  "Arial",
  "Times New Roman",
  "Georgia",
  "Calibri",
  "Cambria",
  "Tahoma",
  "Consolas",
  "Courier New",
];

const CANVAS = 48;
const MATCH_RATIO = 6;
const FIRST_CHARS = 12;

export type UsedFontSource = "specified" | "fallback" | "generic";

export type UsedFontEntry = {
  family: string;
  glyphCount: number;
  source: UsedFontSource;
  sample: string;
  /** CSS generic that resolved to this face, e.g. `serif`. */
  via?: string;
};

export type UsedFontReport = {
  fonts: UsedFontEntry[];
  missingSpecified: string[];
  error?: string;
};

export function parseFontStack(value: string): string[] {
  const parts: string[] = [];
  let cur = "";
  let quote: string | null = null;
  for (const ch of value) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === ",") {
      const t = cur.trim();
      if (t) parts.push(t);
      cur = "";
      continue;
    }
    cur += ch;
  }
  const t = cur.trim();
  if (t) parts.push(t);
  return parts;
}

export function scriptOf(ch: string): string {
  const cp = ch.codePointAt(0) ?? 0;
  if (cp <= 0x24f || (cp >= 0x1e00 && cp <= 0x1eff)) return "latin";
  if (cp >= 0x1100 && cp <= 0x11ff) return "hangul";
  if (cp >= 0x3040 && cp <= 0x30ff) return "kana";
  if (cp >= 0x3100 && cp <= 0x312f) return "bopomofo";
  if (cp >= 0x3400 && cp <= 0x9fff) return "han";
  if (cp >= 0xf900 && cp <= 0xfaff) return "han";
  if (cp >= 0xac00 && cp <= 0xd7af) return "hangul";
  if (cp >= 0x3000 && cp <= 0x303f) return "cjk-punct";
  if (cp >= 0xff00 && cp <= 0xffef) return "cjk-punct";
  if (cp >= 0x20000 && cp <= 0x323af) return "han";
  return "other";
}

export function specifiedFamiliesFromReport(
  values: string[],
  faces: string[] = [],
): string[] {
  const names: string[] = [];
  for (const value of values) {
    for (const family of parseFontStack(value)) {
      if (!isGeneric(family) && !isCssWide(family)) names.push(family);
    }
  }
  for (const face of faces) {
    if (face) names.push(face);
  }
  return unique(names);
}

export function missingAuthorFonts(doc: Document, authorFamilies: string[]): string[] {
  const canvas = doc.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) return [...authorFamilies];
  const loaded = familiesFromDocument(doc);
  return authorFamilies.filter((family) => {
    if (isGeneric(family) || isCssWide(family)) return false;
    if (loaded.some((f) => f.toLowerCase() === family.toLowerCase())) return false;
    return !latinMetricsInstalled(ctx, family);
  });
}

export function firstVisibleChars(doc: Document, n = 12): string {
  const root = doc.body ?? doc.documentElement;
  if (!root) return "";
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let out = "";
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const text = node.nodeValue;
    if (!text) continue;
    const el = node.parentElement;
    if (!el || skipTag(el.tagName)) continue;
    for (const ch of text) {
      if (!ch.trim()) continue;
      out += ch;
      if ([...out].length >= n) return out;
    }
  }
  return out;
}

export function collectUsedFonts(
  doc: Document,
  authorFamilies: string[] = [],
): UsedFontReport {
  const win = doc.defaultView;
  const root = doc.body ?? doc.documentElement;
  if (!win || !root) {
    return { fonts: [], missingSpecified: [] };
  }

  const canvas = doc.createElement("canvas");
  canvas.width = CANVAS;
  canvas.height = CANVAS;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) {
    return { fonts: [], missingSpecified: authorFamilies };
  }

  const installed = new Map<string, boolean>();
  const fpCache = new Map<string, Uint8ClampedArray>();
  const extra = familiesFromDocument(doc);
  const author = unique(authorFamilies.filter((f) => !isGeneric(f) && !isCssWide(f)));

  const probe = (families: string[], ch: string) =>
    fingerprint(ctx, fpCache, fontShorthand("normal", "400", "48px", families), ch);

  const fontInstalled = (family: string) => {
    const hit = installed.get(family);
    if (hit !== undefined) return hit;
    if (isGeneric(family)) {
      installed.set(family, false);
      return false;
    }
    if (extra.some((f) => f.toLowerCase() === family.toLowerCase())) {
      installed.set(family, true);
      return true;
    }
    const ok = latinMetricsInstalled(ctx, family);
    installed.set(family, ok);
    return ok;
  };

  const missingSpecified = author.filter((family) => !fontInstalled(family));
  const missHan = probe([MISSING], "年");
  const missLatin = probe([MISSING], "A");
  // `displayName` normalises family aliases (SimSun → 宋体, Microsoft YaHei →
  // 微软雅黑) into the plane the panel shows; fallback-face comparisons and
  // reported fallback labels both live on that plane so a CSS alias and the
  // resolved face can actually match.
  const cjkFallback = displayName(
    closestInstalled(CJK_FONTS, missHan, "年", fontInstalled, probe) ??
      "（系统 CJK 默认）",
  );
  const latinFallback = displayName(
    closestInstalled(LATIN_FONTS, missLatin, "A", fontInstalled, probe) ??
      "（系统西文默认）",
  );

  const tallies = new Map<
    string,
    { count: number; source: UsedFontSource; first: string; via?: string }
  >();

  /**
   * Which family paints one representative character of a node's script.
   * Canvas fingerprinting dominates this report's cost, so a whole text node
   * is judged through its first character *per script* (Han vs Latin/Kana/…)
   * and the node's per-script character count is credited to that family.
   * Same node + same script + same computed style paints the same face in
   * practice; rare per-glyph fallback differences are rounded away. This
   * turns per-character analysis of a 100k-char chapter into a
   * per-(node, script) one — milliseconds instead of seconds.
   */
  const analyze = (
    stack: string[],
    ch: string,
    script: string,
  ): { family: string; source: UsedFontSource; via?: string } => {
    const actualFont = fontShorthand(
      "normal",
      "400",
      "48px",
      stack.length ? stack : [MISSING],
    );
    const actual = fingerprint(ctx, fpCache, actualFont, ch);
    for (const family of stack) {
      if (isGeneric(family)) {
        if (sameGlyph(actual, probe([family], ch))) {
          return {
            family: `（系统 ${family}）`,
            source: "generic" as const,
          };
        }
        continue;
      }
      if (!fontInstalled(family)) continue;
      const hanLike = isHanLike(script);
      const coverCh = hanLike ? "年" : "A";
      const miss = hanLike ? missHan : missLatin;
      const fallbackName = hanLike ? cjkFallback : latinFallback;
      const paintsOwn = !sameGlyph(probe([family], coverCh), miss);
      // Compare on the display-name plane: 宋体 (written in CSS) and SimSun
      // (the face the engine resolves) must both be recognised as the system
      // fallback face, not skipped as “paints nothing”.
      const isFallbackFace =
        displayName(family).toLowerCase() === fallbackName.toLowerCase();
      if (!paintsOwn && !isFallbackFace) continue;
      if (sameGlyph(actual, probe([family], ch))) {
        return { family: displayName(family), source: "specified" as const };
      }
    }
    return {
      family: isHanLike(script) ? cjkFallback : latinFallback,
      source: "fallback" as const,
    };
  };

  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const text = node.nodeValue;
    if (!text) continue;
    const el = node.parentElement;
    if (!el || skipTag(el.tagName) || !isShown(el, win)) continue;
    const stack = parseFontStack(win.getComputedStyle(el).fontFamily);
    // Bucket the node's visible characters by script, keeping the first
    // character of each bucket as the fingerprint sample.
    const byScript = new Map<string, { count: number; first: string }>();
    for (const ch of text) {
      if (isIgnorable(ch)) continue;
      const script = scriptOf(ch);
      const row = byScript.get(script);
      if (row) {
        row.count += 1;
      } else {
        byScript.set(script, { count: 1, first: ch });
      }
    }
    for (const [script, { count, first }] of byScript) {
      const used = analyze(stack, first, script);
      addTally(tallies, used.family, used.source, used.via, first, count);
    }
  }

  const fonts = [...tallies.entries()]
    .map(([family, v]) => ({
      family,
      glyphCount: v.count,
      source: v.source,
      sample: v.first,
      via: v.via,
    }))
    .sort((a, b) => b.glyphCount - a.glyphCount);

  return {
    fonts,
    missingSpecified,
  };
}

function addTally(
  tallies: Map<
    string,
    { count: number; source: UsedFontSource; first: string; via?: string }
  >,
  family: string,
  source: UsedFontSource,
  via: string | undefined,
  ch: string,
  amount: number,
) {
  let row = tallies.get(family);
  if (!row) {
    row = { count: 0, source, first: "", via };
    tallies.set(family, row);
  }
  row.count += amount;
  if (sourceRank(source) < sourceRank(row.source)) {
    row.source = source;
    if (via) row.via = via;
  }
  if (!row.via && via) row.via = via;
  if ([...row.first].length < FIRST_CHARS) row.first += ch;
}

function isHanLike(script: string): boolean {
  return (
    script === "han" ||
    script === "cjk-punct" ||
    script === "kana" ||
    script === "hangul" ||
    script === "bopomofo"
  );
}

function displayName(family: string): string {
  switch (family.toLowerCase()) {
    case "simsun":
    case "nsimsun":
      return "宋体";
    case "microsoft yahei":
    case "microsoft yahei ui":
      return "微软雅黑";
    case "microsoft jhenghei":
      return "微软正黑体";
    case "kaiti":
      return "楷体";
    case "fangsong":
      return "仿宋";
    case "simhei":
      return "黑体";
    case "dengxian":
      return "等线";
    default:
      return family;
  }
}

function sourceRank(s: UsedFontSource): number {
  if (s === "specified") return 0;
  if (s === "generic") return 1;
  return 2;
}

function familiesFromDocument(doc: Document): string[] {
  const out: string[] = [];
  try {
    doc.fonts?.forEach((face) => {
      if (face.status && face.status !== "loaded") return;
      if (face.family) out.push(face.family.replace(/^["']|["']$/g, ""));
    });
  } catch {
    /* FontFaceSet may be missing */
  }
  return out;
}

function skipTag(tag: string): boolean {
  const t = tag.toLowerCase();
  return t === "script" || t === "style" || t === "noscript" || t === "textarea";
}

function isShown(el: Element, win: Window): boolean {
  const s = win.getComputedStyle(el);
  return s.display !== "none" && s.visibility !== "hidden" && s.fontSize !== "0px";
}

function isIgnorable(ch: string): boolean {
  return ch.trim().length === 0;
}

function isGeneric(family: string): boolean {
  return GENERIC.has(family.toLowerCase());
}

function isCssWide(family: string): boolean {
  return /^(inherit|initial|unset|revert|revert-layer)$/i.test(family.trim());
}

function quoteFamily(family: string): string {
  if (isGeneric(family) || family === MISSING) return family;
  return `"${family.replace(/"/g, '\\"')}"`;
}

/** True only if this family actually changes Latin metrics vs CSS generics.
 *  Do not test with Han: missing CJK names still fall back to YaHei and look "installed". */
function latinMetricsInstalled(ctx: CanvasRenderingContext2D, family: string): boolean {
  const sample = "mmmmmmmmlliIiWw@#%";
  for (const generic of ["monospace", "serif", "sans-serif"]) {
    ctx.font = `72px ${generic}`;
    const baseline = ctx.measureText(sample).width;
    ctx.font = `72px ${quoteFamily(family)}, ${generic}`;
    const mixed = ctx.measureText(sample).width;
    if (Math.abs(mixed - baseline) > 0.75) return true;
  }
  return false;
}

function fontShorthand(
  style: string,
  weight: string,
  size: string,
  families: string[],
): string {
  return `${style} ${weight} ${size} ${families.map(quoteFamily).join(", ")}`;
}

function fingerprint(
  ctx: CanvasRenderingContext2D,
  cache: Map<string, Uint8ClampedArray>,
  font: string,
  ch: string,
): Uint8ClampedArray {
  const key = `${font}\n${ch}`;
  const hit = cache.get(key);
  if (hit) return hit;
  ctx.clearRect(0, 0, CANVAS, CANVAS);
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, CANVAS, CANVAS);
  ctx.fillStyle = "#000";
  ctx.font = font;
  ctx.textBaseline = "top";
  ctx.fillText(ch, 4, 4);
  const data = ctx.getImageData(0, 0, CANVAS, CANVAS).data;
  const copy = new Uint8ClampedArray(data);
  cache.set(key, copy);
  return copy;
}

function distance(a: Uint8ClampedArray, b: Uint8ClampedArray): number {
  const n = Math.min(a.length, b.length);
  let d = 0;
  for (let i = 0; i < n; i++) d += Math.abs(a[i] - b[i]);
  return d;
}

function sameGlyph(a: Uint8ClampedArray, b: Uint8ClampedArray): boolean {
  return distance(a, b) <= a.length * MATCH_RATIO;
}

function closestInstalled(
  list: string[],
  miss: Uint8ClampedArray,
  ch: string,
  fontInstalled: (family: string) => boolean,
  probe: (families: string[], ch: string) => Uint8ClampedArray,
): string | null {
  let best: { family: string; dist: number } | null = null;
  for (const family of list) {
    if (!fontInstalled(family)) continue;
    const dist = distance(probe([family], ch), miss);
    if (!best || dist < best.dist) best = { family, dist };
  }
  if (best && best.dist <= miss.length * MATCH_RATIO * 4) return best.family;
  return null;
}

function unique(items: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const item of items) {
    if (seen.has(item)) continue;
    seen.add(item);
    out.push(item);
  }
  return out;
}
