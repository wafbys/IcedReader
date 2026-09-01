import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import {
  chapterIndex,
  type SpineItem,
  type TocNode,
} from "./types";

type Props = {
  toc: TocNode[];
  spine: SpineItem[];
  currentIndex: number;
  onSelect: (href: string) => void;
  onClose: () => void;
};

function spineAsToc(spine: SpineItem[]): TocNode[] {
  return spine.map((item) => ({
    label: item.title?.trim() || item.href,
    href: item.href,
    children: [],
  }));
}

function allOpenIds(nodes: TocNode[], prefix = ""): Set<string> {
  const open = new Set<string>();
  nodes.forEach((node, i) => {
    const id = `${prefix}${i}`;
    if (node.children.length) {
      open.add(id);
      allOpenIds(node.children, `${id}.`).forEach((child) => open.add(child));
    }
  });
  return open;
}

export default function TocPanel({
  toc,
  spine,
  currentIndex,
  onSelect,
  onClose,
}: Props) {
  const tree = useMemo(
    () => (toc.length ? toc : spineAsToc(spine)),
    [toc, spine],
  );
  const [open, setOpen] = useState(() => allOpenIds(tree));
  const currentRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    setOpen(allOpenIds(tree));
  }, [tree]);

  useEffect(() => {
    currentRef.current?.scrollIntoView({ block: "nearest" });
  }, [currentIndex, tree]);

  if (!tree.length) {
    return (
      <aside className="toc-drawer" aria-label="目录">
        <div className="toc-head">
          <strong>目录</strong>
          <button type="button" className="btn ghost small" onClick={onClose}>
            关闭
          </button>
        </div>
        <p className="toc-empty">这本书没有目录。</p>
      </aside>
    );
  }

  return (
    <aside className="toc-drawer" aria-label="目录">
      <div className="toc-head">
        <strong>目录</strong>
        <button type="button" className="btn ghost small" onClick={onClose}>
          关闭
        </button>
      </div>
      <nav className="toc-nav">
        <TocBranch
          nodes={tree}
          prefix=""
          depth={0}
          spine={spine}
          currentIndex={currentIndex}
          open={open}
          currentRef={currentRef}
          onToggle={(id) => {
            setOpen((prev) => {
              const next = new Set(prev);
              if (next.has(id)) next.delete(id);
              else next.add(id);
              return next;
            });
          }}
          onSelect={onSelect}
        />
      </nav>
    </aside>
  );
}

function TocBranch({
  nodes,
  prefix,
  depth,
  spine,
  currentIndex,
  open,
  currentRef,
  onToggle,
  onSelect,
}: {
  nodes: TocNode[];
  prefix: string;
  depth: number;
  spine: SpineItem[];
  currentIndex: number;
  open: Set<string>;
  currentRef: RefObject<HTMLButtonElement | null>;
  onToggle: (id: string) => void;
  onSelect: (href: string) => void;
}) {
  return (
    <ul className="toc-list" style={{ paddingLeft: depth ? 12 : 0 }}>
      {nodes.map((node, i) => {
        const id = `${prefix}${i}`;
        const hasKids = node.children.length > 0;
        const expanded = hasKids && open.has(id);
        const active =
          !!node.href && chapterIndex(spine, node.href) === currentIndex;
        return (
          <li key={id}>
            <div className={`toc-row${active ? " current" : ""}`}>
              {hasKids ? (
                <button
                  type="button"
                  className="toc-twist"
                  aria-expanded={expanded}
                  aria-label={expanded ? "折叠" : "展开"}
                  onClick={() => onToggle(id)}
                >
                  {expanded ? "▾" : "▸"}
                </button>
              ) : (
                <span className="toc-twist spacer" />
              )}
              <button
                type="button"
                className="toc-label"
                ref={active ? currentRef : undefined}
                disabled={!node.href}
                onClick={() => {
                  if (node.href) onSelect(node.href);
                }}
              >
                {node.label || "未命名"}
              </button>
            </div>
            {expanded && (
              <TocBranch
                nodes={node.children}
                prefix={`${id}.`}
                depth={depth + 1}
                spine={spine}
                currentIndex={currentIndex}
                open={open}
                currentRef={currentRef}
                onToggle={onToggle}
                onSelect={onSelect}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}
