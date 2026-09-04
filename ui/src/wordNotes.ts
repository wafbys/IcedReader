/**
 * Word-note presentation styles.
 *
 * The Rust format layer turns `data-wr-footernote` word notes (WeRead-export
 * style EPUBs, e.g. 《资治通鉴全本注译》) into two things:
 *   - an empty `<a id="wr-note-back-N" class="wr-note" data-label="…" data-note="…"
 *     href="…x.xhtml#wr-note-N">` marker right after the annotated word —
 *     hovering it shows a parent-page dark bubble with the full note
 *     (`data-note` carries the text; the marker has no `title`, so no native
 *     tooltip competes), clicking jumps to the note block;
 *   - a trailing `<div class="wr-notes">` per paragraph holding the full note
 *     text as anchor targets; each note starts with its `[n]` label rendered
 *     as a `<a class="wr-note-back">` link back to the marker (same shape as
 *     the `[n]` ↔ note list of an ordinary annotated EPUB, e.g. 东周列国志).
 *     Both links are absolute same-document URLs so the reader routes them as
 *     same-file anchors even when the note block crosses a page break.
 * Note items refuse column breaks (`break-inside: avoid`) so they stay whole
 * like printed footnotes; an item too tall for a single column still splits,
 * and the item then carries a textless trailing back link (drawn via CSS)
 * that the parent page reveals through the `wr-note-cross` class — so the
 * continuation page always has a way back to the marker.
 * The marker itself carries no text (the number is drawn via CSS `::after`),
 * so the chapter's text-node sequence only gains the note blocks — stably on
 * every render — and highlight anchoring keeps working.
 *
 * This style is injected by the parent page into the chapter head, next to the
 * flow / highlight styles; the chapter keeps its sandbox without scripts.
 */

export const WORD_NOTE_STYLE_ID = "iced-reader-note-style";

const WORD_NOTE_CSS = `
a.wr-note {
  text-decoration: none;
  color: inherit;
  cursor: pointer;
}
a.wr-note::after {
  content: "[" attr(data-label) "]";
  font-size: 0.62em;
  line-height: 1;
  vertical-align: super;
  color: #a0672f;
}
div.wr-notes {
  font-size: 0.9em;
  color: #707070;
  margin: 0.1em 0 0.5em 0;
}
div.wr-notes p.wr-note-item {
  margin: 0.15em 0;
  text-indent: 0;
  /* A footnote-style item must not be sawn in half by a column break; when
     the current column lacks room the whole item moves to the next one. */
  break-inside: avoid;
  page-break-inside: avoid;
}
div.wr-notes a.wr-note-back {
  color: #a0672f;
  text-decoration: none;
  margin-right: 0.35em;
  cursor: pointer;
}
div.wr-notes a.wr-note-back:hover {
  color: #7c4a1c;
  text-decoration: underline;
}
/* Trailing back link of an item that a column break really split (parent
   page adds .wr-note-cross to the item; the link text is CSS-drawn, so no
   text node shifts highlight anchoring). Hidden while the item is whole. */
div.wr-notes a.wr-note-back-tail {
  display: none;
  margin-left: 0.6em;
}
div.wr-notes p.wr-note-cross a.wr-note-back-tail {
  display: inline;
}
div.wr-notes a.wr-note-back-tail::after {
  content: "↩返回";
  font-size: 0.85em;
}`;

/** Inject the note style when the chapter document contains word notes. */
export function ensureWordNoteStyle(doc: Document): void {
  if (!doc.querySelector("a.wr-note, .wr-notes")) return;
  let style = doc.getElementById(WORD_NOTE_STYLE_ID) as HTMLStyleElement | null;
  if (!style) {
    style = doc.createElement("style");
    style.id = WORD_NOTE_STYLE_ID;
    doc.head.appendChild(style);
  }
  style.textContent = WORD_NOTE_CSS;
}
