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

export type PlatformFontUsage = {
  familyName: string;
  glyphCount: number;
  isCustomFont: boolean;
};

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

export function reportFromPlatform(
  platform: PlatformFontUsage[],
  authorFamilies: string[],
  firstChars: string,
  missingSpecified: string[],
): UsedFontReport {
  const author = new Set(authorFamilies.map((f) => f.toLowerCase()));
  const fonts = platform
    .filter((p) => p.glyphCount > 0)
    .map((p) => ({
      family: p.familyName,
      glyphCount: p.glyphCount,
      source: (p.isCustomFont || author.has(p.familyName.toLowerCase())
        ? "specified"
        : "fallback") as UsedFontSource,
      sample: "",
    }))
    .sort((a, b) => b.glyphCount - a.glyphCount);
  if (fonts[0] && firstChars) fonts[0].sample = firstChars;
  return { fonts, missingSpecified };
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
  const cjkFallback =
    closestInstalled(CJK_FONTS, missHan, "年", fontInstalled, probe) ?? "（系统 CJK 默认）";
  const latinFallback =
    closestInstalled(LATIN_FONTS, missLatin, "A", fontInstalled, probe) ?? "（系统西文默认）";

  const tallies = new Map<
    string,
    { count: number; source: UsedFontSource; first: string }
  >();

  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const text = node.nodeValue;
    if (!text) continue;
    const el = node.parentElement;
    if (!el || skipTag(el.tagName) || !isShown(el, win)) continue;
    const cs = win.getComputedStyle(el);
    const stack = parseFontStack(cs.fontFamily);
    const actualFont = fontShorthand("normal", "400", "48px", stack.length ? stack : [MISSING]);
    for (const ch of text) {
      if (isIgnorable(ch)) continue;
      const script = scriptOf(ch);
      const actual = fingerprint(ctx, fpCache, actualFont, ch);
      let used: { family: string; source: UsedFontSource } | undefined;
      for (const family of stack) {
        if (isGeneric(family) || !fontInstalled(family) || !coversScript(family, script)) {
          continue;
        }
        if (sameGlyph(actual, probe([family], ch))) {
          used = { family, source: "specified" };
          break;
        }
      }
      if (!used) {
        const han =
          script === "han" ||
          script === "cjk-punct" ||
          script === "kana" ||
          script === "hangul" ||
          script === "bopomofo";
        used = {
          family: han ? cjkFallback : latinFallback,
          source: "fallback",
        };
      }
      addTally(tallies, used.family, used.source, ch);
    }
  }

  const fonts = [...tallies.entries()]
    .map(([family, v]) => ({
      family,
      glyphCount: v.count,
      source: v.source,
      sample: v.first,
    }))
    .sort((a, b) => b.glyphCount - a.glyphCount);

  return {
    fonts,
    missingSpecified,
  };
}

function addTally(
  tallies: Map<string, { count: number; source: UsedFontSource; first: string }>,
  family: string,
  source: UsedFontSource,
  ch: string,
) {
  let row = tallies.get(family);
  if (!row) {
    row = { count: 0, source, first: "" };
    tallies.set(family, row);
  }
  row.count += 1;
  if (sourceRank(source) < sourceRank(row.source)) row.source = source;
  if ([...row.first].length < FIRST_CHARS) row.first += ch;
}

function sourceRank(s: UsedFontSource): number {
  if (s === "specified") return 0;
  if (s === "fallback") return 1;
  return 2;
}

function familiesFromDocument(doc: Document): string[] {
  const out: string[] = [];
  try {
    doc.fonts?.forEach((face) => {
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

function coversScript(family: string, script: string): boolean {
  const n = family.toLowerCase();
  const cjk =
    /icedreader|yahei|jhenghei|simsun|nsimsun|simhei|kaiti|fangsong|dengxian|mingliu|pmingliu|pingfang|stheiti|stsong|stkaiti|source han|noto.*(cjk|sc|tc)|hiragino|songti|heiti|kaiti|fangzheng|^fz[a-z]|华文|思源|黑体|宋体|楷体|微软/.test(
      n,
    );
  if (script === "han" || script === "cjk-punct" || script === "bopomofo") return cjk;
  if (script === "kana") {
    return cjk || /yu gothic|meiryo|ms gothic|ms mincho|hiragino|noto.*jp/.test(n);
  }
  if (script === "hangul") {
    return cjk || /malgun|gulim|dotum|batang|noto.*kr/.test(n);
  }
  return true;
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
