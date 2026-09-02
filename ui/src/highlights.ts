/**
 * Highlight anchoring & painting, all operating on the chapter iframe's own
 * Document from the parent page (the chapter keeps sandbox without scripts).
 *
 * Anchoring model
 * ---------------
 * Chapter HTML is regenerated deterministically on every open, so the DOM's
 * text nodes appear in the same order each time. A highlight is stored as the
 * half-open span [start, end) over that sequence: `startText`/`endText` are
 * indexes into `collectTexts(doc)`, plus an in-node offset. `text` keeps a
 * whitespace-normalised excerpt so a changed/regenerated edition can be
 * re-anchored by searching it instead of silently drifting; records that
 * cannot be re-anchored are skipped but kept on disk.
 *
 * Painting
 * --------
 * Highlights are painted with the CSS Custom Highlight API (Chromium 105+,
 * which WebView2 ships). Nothing is injected into the chapter DOM — no <mark>,
 * no layout change — the colour lives in `::highlight(iced-reader-highlight)`
 * in the chapter document's head, alongside the pagination flow style.
 */

import type { HighlightRecord } from "./types";

/** CSS.highlights registry name for all highlights of one chapter. */
export const HIGHLIGHT_NAME = "iced-reader-highlight";
/** <style id=...> injected into the chapter head, next to the flow style. */
export const HIGHLIGHT_STYLE_ID = "iced-reader-highlight-style";

/** Excerpt length kept for validation/re-anchoring. */
export const EXCERPT_MAX = 160;
/** When an excerpt is longer than EXCERPT_MAX keep head and tail only. */
const EXCERPT_HEAD = 80;
const EXCERPT_TAIL = 80;

export type TextPoint = { seq: number; offset: number };

export type HighlightAnchor = {
  start: TextPoint;
  end: TextPoint;
  text: string;
};

/** A highlight span expressed over the concatenated chapter text (chars). */
export type AppliedSpan = { id: string; from: number; to: number };

export function collectTexts(doc: Document): Text[] {
  const texts: Text[] = [];
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const parent = node.parentElement;
      if (parent && /^(SCRIPT|STYLE|TEMPLATE|NOSCRIPT)$/i.test(parent.tagName)) {
        return NodeFilter.FILTER_REJECT;
      }
      return NodeFilter.FILTER_ACCEPT;
    },
  });
  let node = walker.nextNode();
  while (node) {
    texts.push(node as Text);
    node = walker.nextNode();
  }
  return texts;
}

/** prefix[i] = total chars before texts[i]; prefix[texts.length] = total. */
export function textPrefix(texts: Text[]): number[] {
  const prefix = new Array<number>(texts.length + 1);
  let acc = 0;
  for (let i = 0; i < texts.length; i++) {
    prefix[i] = acc;
    acc += texts[i].data.length;
  }
  prefix[texts.length] = acc;
  return prefix;
}

export function charOfPoint(prefix: number[], point: TextPoint): number {
  return prefix[point.seq] + point.offset;
}

/** Convert a global char index back to (seq, offset); null when out of range. */
export function pointOfChar(
  texts: Text[],
  prefix: number[],
  char: number,
): TextPoint | null {
  const total = prefix[texts.length];
  const c = Math.min(Math.max(0, char), total);
  let lo = 0;
  let hi = texts.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (prefix[mid] <= c && c < prefix[mid] + texts[mid].data.length) {
      return { seq: mid, offset: c - prefix[mid] };
    }
    if (c < prefix[mid]) {
      hi = mid;
    } else {
      lo = mid + 1;
    }
  }
  return null;
}

export function normalizeText(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** Concatenated plain text of the chapter, node data in order. */
export function plainText(texts: Text[]): string {
  let out = "";
  for (const t of texts) out += t.data;
  return out;
}

/**
 * Turn a user selection range into an anchor over the text sequence.
 * Range end points normally sit inside text nodes; when they fall on an
 * element boundary (whole-block selections) we snap forward/backward to the
 * nearest text. Returns null when there is no usable text (e.g. an image).
 */
export function anchorFromRange(
  doc: Document,
  range: Range,
): HighlightAnchor | null {
  const texts = collectTexts(doc);
  const start = snapPoint(doc, texts, range.startContainer, range.startOffset, "start");
  const end = snapPoint(doc, texts, range.endContainer, range.endOffset, "end");
  if (!start || !end) return null;
  const prefix = textPrefix(texts);
  const from = charOfPoint(prefix, start);
  const to = charOfPoint(prefix, end);
  if (to <= from) return null;
  const raw = range.toString();
  const norm = normalizeText(raw);
  const text =
    norm.length <= EXCERPT_MAX
      ? norm
      : `${norm.slice(0, EXCERPT_HEAD)}…${norm.slice(-EXCERPT_TAIL)}`;
  if (!text) return null;
  return { start, end, text };
}

type SnapDir = "start" | "end";

/** First text node in document order at/after `node` (incl. its subtree). */
function forwardTextFrom(doc: Document, node: Node | null): Text | null {
  if (!node) return null;
  if (node.nodeType === Node.TEXT_NODE) return node as Text;
  const walker = doc.createTreeWalker(node, NodeFilter.SHOW_TEXT);
  const found = walker.nextNode() as Text | null;
  if (found) return found;
  return forwardTextFrom(doc, node.nextSibling);
}

/** Last text node in document order at/before `node` (incl. its subtree). */
function backwardTextTo(doc: Document, node: Node | null): Text | null {
  if (!node) return null;
  if (node.nodeType === Node.TEXT_NODE) return node as Text;
  const walker = doc.createTreeWalker(node, NodeFilter.SHOW_TEXT);
  let last: Text | null = null;
  let cur = walker.nextNode() as Text | null;
  while (cur) {
    last = cur;
    cur = walker.nextNode() as Text | null;
  }
  if (last) return last;
  return backwardTextTo(doc, node.previousSibling);
}

function indexOfText(texts: Text[], node: Text): number {
  return texts.indexOf(node);
}

/**
 * Snap a Range end point (container, offset) into a concrete text position.
 * - Text container: use it directly (offset clamped).
 * - Element container: `offset` indexes child nodes. For the start we jump to
 *   the first text inside/after `childNodes[offset]` (or, when offset is at
 *   the element's end, to the text after the element). For the end we jump to
 *   the end of the text that precedes `childNodes[offset]` (offset 0 means
 *   the element's start: text before it).
 * Returns null when the boundary has no adjacent text at all.
 */
function snapPoint(
  doc: Document,
  texts: Text[],
  container: Node,
  offset: number,
  dir: SnapDir,
): TextPoint | null {
  if (container.nodeType === Node.TEXT_NODE) {
    const text = container as Text;
    const seq = indexOfText(texts, text);
    if (seq < 0) return null;
    const len = text.data.length;
    if (dir === "end") {
      return { seq, offset: Math.min(Math.max(0, offset), len) };
    }
    return { seq, offset: Math.min(Math.max(0, offset), len) };
  }
  if (container.nodeType !== Node.ELEMENT_NODE) return null;
  const el = container as Element;
  if (dir === "start") {
    const child = offset < el.childNodes.length ? el.childNodes[offset] : null;
    const text = child
      ? forwardTextFrom(doc, child)
      : forwardTextFrom(doc, el.nextSibling);
    if (!text) return null;
    const seq = indexOfText(texts, text);
    if (seq < 0) return null;
    return { seq, offset: 0 };
  }
  // end: boundary sits before childNodes[offset]; snap to the end of the text
  // ending there. offset 0 means the selection ends where el starts.
  const prev = offset > 0 ? el.childNodes[offset - 1] : null;
  const text = prev
    ? backwardTextTo(doc, prev)
    : backwardTextTo(doc, el.previousSibling);
  if (!text) return null;
  const seq = indexOfText(texts, text);
  if (seq < 0) return null;
  return { seq, offset: text.data.length };
}

/** Half-open char interval [from, to) of a record, or null when out of range. */
function recordSpan(
  texts: Text[],
  prefix: number[],
  rec: HighlightRecord,
): { from: number; to: number } | null {
  if (rec.startText < 0 || rec.endText < 0 || rec.startText >= texts.length || rec.endText >= texts.length) {
    return null;
  }
  const startNode = texts[rec.startText];
  const endNode = texts[rec.endText];
  const from = prefix[rec.startText] + Math.min(rec.startOffset, startNode.data.length);
  const to = prefix[rec.endText] + Math.min(rec.endOffset, endNode.data.length);
  if (to <= from) return null;
  return { from, to };
}

/**
 * Re-anchor a record whose text-node indexes no longer fit (book replaced or
 * regenerated) by searching its excerpt. Looks for the whole excerpt first,
 * then head+tail. Returns the char interval, or null when unrecoverable.
 */
function anchorByExcerpt(
  texts: Text[],
  rec: HighlightRecord,
): { from: number; to: number } | null {
  const excerpt = normalizeText(rec.text);
  if (excerpt.length < 2) return null;
  const docText = plainText(texts);
  if (docText.length === 0) return null;

  const headLen = Math.min(24, excerpt.length);
  const head = excerpt.slice(0, headLen);
  let at = docText.indexOf(head);
  if (at < 0) {
    // Maybe whitespace differs inside the head (rare); fall back to word start.
    const word = head.split(" ")[0];
    if (word.length >= 4) {
      at = docText.indexOf(word);
    }
  }
  if (at < 0) return null;

  // Whole excerpt match (whitespace-normalised lines usually match exactly).
  const whole = docText.indexOf(excerpt, at);
  if (whole >= 0) {
    return { from: whole, to: whole + excerpt.length };
  }
  // Tail match after head.
  const tailLen = Math.min(24, excerpt.length);
  const tail = excerpt.slice(-tailLen);
  const tailAt = docText.indexOf(tail, at + headLen);
  if (tailAt > at) {
    return { from: at, to: tailAt + tail.length };
  }
  return { from: at, to: at + headLen };
}

export type RangeResult = {
  ranges: Range[];
  applied: AppliedSpan[];
  missing: number;
};

/** Turn stored records into live Range objects (skip the unrecoverable). */
export function recordsToRanges(
  doc: Document,
  records: HighlightRecord[],
): RangeResult {
  const texts = collectTexts(doc);
  const prefix = textPrefix(texts);
  const ranges: Range[] = [];
  const applied: AppliedSpan[] = [];
  let missing = 0;
  for (const rec of records) {
    let span = recordSpan(texts, prefix, rec);
    if (!span) {
      const byExcerpt = anchorByExcerpt(texts, rec);
      if (byExcerpt) {
        span = byExcerpt;
      }
    }
    if (!span) {
      missing += 1;
      continue;
    }
    const fromPt = pointOfChar(texts, prefix, span.from);
    const toPt = pointOfChar(texts, prefix, span.to);
    if (!fromPt || !toPt || span.to <= span.from) {
      missing += 1;
      continue;
    }
    const range = doc.createRange();
    range.setStart(texts[fromPt.seq], fromPt.offset);
    range.setEnd(texts[toPt.seq], toPt.offset);
    ranges.push(range);
    applied.push({ id: rec.id, from: span.from, to: span.to });
  }
  return { ranges, applied, missing };
}

export function highlightSupported(doc: Document): boolean {
  const win = doc.defaultView as
    | (Window & { CSS?: { highlights?: unknown }; Highlight?: unknown })
    | null;
  return !!(win?.CSS?.highlights && win.Highlight);
}

/**
 * Rebuild the full highlight overlay for this chapter document.
 * Returns the painted spans (for hit-testing), or null when the WebView does
 * not support the CSS Custom Highlight API.
 */
export function paintHighlights(
  doc: Document,
  records: HighlightRecord[],
): AppliedSpan[] | null {
  const win = doc.defaultView as
    | (Window & {
        CSS?: {
          highlights?: { delete(name: string): void; set(name: string, hl: unknown): void };
        };
        Highlight?: new (...ranges: Range[]) => unknown;
      })
    | null;
  const registry = win?.CSS?.highlights;
  const HighlightCtor = win?.Highlight;
  if (!registry || !HighlightCtor) return null;

  // Ensure ::highlight(iced-reader-highlight) styling exists in this doc.
  let styleEl = doc.getElementById(HIGHLIGHT_STYLE_ID) as HTMLStyleElement | null;
  if (!styleEl) {
    styleEl = doc.createElement("style");
    styleEl.id = HIGHLIGHT_STYLE_ID;
    doc.head.appendChild(styleEl);
  }
  styleEl.textContent = `
::highlight(${HIGHLIGHT_NAME}) {
  background-color: rgba(255, 208, 96, 0.55);
  color: inherit;
}`;

  const { ranges, applied } = recordsToRanges(doc, records);
  registry.delete(HIGHLIGHT_NAME);
  if (ranges.length > 0) {
    registry.set(HIGHLIGHT_NAME, new HighlightCtor(...ranges));
  }
  return applied;
}

/**
 * Character offset under (x, y) inside the chapter document viewport, or null.
 * Used to detect a click on an existing highlight (selection is empty).
 */
export function charAtPoint(doc: Document, x: number, y: number): number | null {
  const caret = doc.caretRangeFromPoint(x, y);
  if (!caret) return null;
  const texts = collectTexts(doc);
  const point = snapPoint(doc, texts, caret.startContainer, caret.startOffset, "start");
  if (!point) return null;
  const prefix = textPrefix(texts);
  return charOfPoint(prefix, point);
}

/** First applied span containing the char offset. */
export function spanAtChar(
  applied: AppliedSpan[],
  char: number,
): AppliedSpan | null {
  if (char < 0) return null;
  for (const span of applied) {
    if (char >= span.from && char < span.to) return span;
  }
  return null;
}

/** Char interval of a fresh anchor over the current chapter text. */
export function anchorSpan(
  texts: Text[],
  anchor: HighlightAnchor,
): { from: number; to: number } | null {
  const prefix = textPrefix(texts);
  const from = charOfPoint(prefix, anchor.start);
  const to = charOfPoint(prefix, anchor.end);
  if (to <= from) return null;
  return { from, to };
}

export type AnchorOverlapInfo = {
  /** Records that intersect the fresh anchor. */
  overlapIds: string[];
  /** When exactly one existing record fully contains the fresh anchor. */
  containedId: string | null;
};

/**
 * Overlap analysis of a fresh anchor against stored records on this doc.
 * When the selection sits entirely inside one existing highlight (double-click
 * on an already-highlighted word), `containedId` names it so the toolbar can
 * offer deleting it instead of refusing a new highlight.
 */
export function anchorOverlapInfo(
  doc: Document,
  records: HighlightRecord[],
  anchor: HighlightAnchor,
): AnchorOverlapInfo {
  const texts = collectTexts(doc);
  const prefix = textPrefix(texts);
  const mine = anchorSpan(texts, anchor);
  if (!mine) return { overlapIds: [], containedId: null };
  const overlapIds: string[] = [];
  let containedId: string | null = null;
  for (const rec of records) {
    const span = recordSpan(texts, prefix, rec);
    if (!span) continue;
    if (mine.from < span.to && span.from < mine.to) {
      overlapIds.push(rec.id);
      if (span.from <= mine.from && mine.to <= span.to) containedId = rec.id;
    }
  }
  if (overlapIds.length !== 1) containedId = null;
  return { overlapIds, containedId };
}
