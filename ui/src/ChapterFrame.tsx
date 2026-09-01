import { useEffect, useRef } from "react";
import { collectUsedFonts, type UsedFontReport } from "./usedFonts";

type Props = {
  html: string;
  restoreFraction: number;
  authorFamilies: string[];
  documentLang?: string | null;
  onProgress: (fraction: number) => void;
  onUsedFonts: (report: UsedFontReport) => void;
};

function usableLang(value: string | null | undefined): string | null {
  if (!value) return null;
  const t = value.trim();
  if (!t || /^und$/i.test(t) || /^zxx$/i.test(t)) return null;
  if (!/^[\w-]+$/.test(t)) return null;
  return t;
}

/** HTML parsers ignore xml:lang; Blink uses `lang` for generic serif/sans CJK. */
export function ensureHtmlLang(html: string, bookLang?: string | null): string {
  if (/\slang\s*=/i.test(html)) return html;
  const xml = html.match(/\sxml:lang\s*=\s*["']([^"']+)["']/i);
  const lang = usableLang(xml?.[1]) ?? usableLang(bookLang);
  if (!lang) return html;
  if (/<html\b/i.test(html)) {
    return html.replace(/<html\b/i, `<html lang="${lang}"`);
  }
  return html;
}

function scrollingRoot(doc: Document): Element {
  return doc.scrollingElement ?? doc.documentElement;
}

function setFraction(doc: Document, fraction: number) {
  const el = scrollingRoot(doc);
  const max = el.scrollHeight - el.clientHeight;
  el.scrollTop = max > 0 ? Math.min(1, Math.max(0, fraction)) * max : 0;
}

function readFraction(doc: Document): number {
  const el = scrollingRoot(doc);
  const max = el.scrollHeight - el.clientHeight;
  if (max <= 0) return 0;
  return Math.min(1, Math.max(0, el.scrollTop / max));
}

export default function ChapterFrame({
  html,
  restoreFraction,
  authorFamilies,
  documentLang,
  onProgress,
  onUsedFonts,
}: Props) {
  const ref = useRef<HTMLIFrameElement>(null);
  const restored = useRef(false);
  const gen = useRef(0);
  const onProgressRef = useRef(onProgress);
  onProgressRef.current = onProgress;
  const onUsedFontsRef = useRef(onUsedFonts);
  onUsedFontsRef.current = onUsedFonts;
  const authorRef = useRef(authorFamilies);
  authorRef.current = authorFamilies;

  useEffect(() => {
    restored.current = false;
  }, [html, restoreFraction]);

  return (
    <iframe
      ref={ref}
      id="iced-chapter"
      className="chapter"
      title="chapter"
      srcDoc={ensureHtmlLang(html, documentLang)}
      sandbox="allow-same-origin allow-popups-to-escape-sandbox"
      onLoad={() => {
        const doc = ref.current?.contentDocument;
        if (!doc) return;
        const apply = () => setFraction(doc, restoreFraction);
        if (!restored.current) {
          apply();
          requestAnimationFrame(() => {
            apply();
            restored.current = true;
          });
        }
        const report = () => onProgressRef.current(readFraction(doc));
        doc.addEventListener("scroll", report, { passive: true, capture: true });
        doc.defaultView?.addEventListener("scroll", report, { passive: true });
        const token = ++gen.current;
        const publishUsed = () => {
          if (token !== gen.current) return;
          onUsedFontsRef.current(collectUsedFonts(doc, authorRef.current));
        };
        const ready = doc.fonts?.ready ?? Promise.resolve();
        void ready.then(() => {
          requestAnimationFrame(publishUsed);
          window.setTimeout(publishUsed, 450);
        });
      }}
    />
  );
}
