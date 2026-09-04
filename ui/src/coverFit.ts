/**
 * Whole-page background covers (e.g. 《资治通鉴全本注译》's opening page,
 * `.cover-page { background: url(../cover1.png); background-size: cover; }`).
 *
 * The book paints its cover as a *body background* with `cover` sizing, so on
 * a landscape / short viewport the tall artwork is scaled by width and the
 * top & bottom get cropped away — the reader only sees a colour band, not the
 * cover. When a chapter is essentially just such a background image (nearly
 * no body text), the parent page switches that body to `contain`: the whole
 * artwork then fits inside the column/viewport with paper showing around it.
 *
 * The check is deliberately narrow so ordinary prose and decorative
 * backgrounds are never touched:
 *   - body text is ≤ MAX_BODY_TEXT chars (whitespace stripped); and
 *   - the body carries a real background *image* (not `none`, not a colour).
 * `background-size/repeat/position` are overridden only; the image itself and
 * every other book rule stay as authored.
 */

export const COVER_FIT_STYLE_ID = "iced-reader-cover-fit";
/** Body text above this length is real prose — never a pure cover page. */
const MAX_BODY_TEXT = 80;

const COVER_FIT_CSS = `body {
  background-size: contain !important;
  background-repeat: no-repeat !important;
  background-position: center !important;
}`;

/** Inject the contain rule when this document looks like a whole-page cover. */
export function ensureCoverFit(doc: Document): void {
  const body = doc.body;
  if (!body) return;
  const text = (body.textContent ?? "").replace(/\s+/g, "");
  if (text.length > MAX_BODY_TEXT) return;
  const win = doc.defaultView;
  if (!win) return;
  const bg = win.getComputedStyle(body).backgroundImage;
  if (!bg || bg === "none") return;
  let style = doc.getElementById(COVER_FIT_STYLE_ID) as HTMLStyleElement | null;
  if (!style) {
    style = doc.createElement("style");
    style.id = COVER_FIT_STYLE_ID;
    doc.head.appendChild(style);
  }
  style.textContent = COVER_FIT_CSS;
}
