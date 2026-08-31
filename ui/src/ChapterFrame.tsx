import { useEffect, useRef } from "react";

type Props = {
  html: string;
  restoreFraction: number;
  onProgress: (fraction: number) => void;
};

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

export default function ChapterFrame({ html, restoreFraction, onProgress }: Props) {
  const ref = useRef<HTMLIFrameElement>(null);
  const restored = useRef(false);
  const onProgressRef = useRef(onProgress);
  onProgressRef.current = onProgress;

  useEffect(() => {
    restored.current = false;
  }, [html, restoreFraction]);

  return (
    <iframe
      ref={ref}
      className="chapter"
      title="chapter"
      srcDoc={html}
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
      }}
    />
  );
}
