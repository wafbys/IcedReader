/**
 * Word-note presentation styles.
 *
 * The Rust format layer turns `data-wr-footernote` word notes (WeRead-export
 * style EPUBs, e.g. 《资治通鉴全本注译》) into two things:
 *   - an empty `<a class="wr-note" data-label="…" title="…" href="#wr-note-N">`
 *     marker right after the annotated word — hovering it shows the full note
 *     through the native tooltip, clicking jumps to the note block;
 *   - a trailing `<div class="wr-notes">` per paragraph holding the full note
 *     text as anchor targets.
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
}
div.wr-notes .wr-note-no {
  color: #a0672f;
  margin-right: 0.35em;
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
