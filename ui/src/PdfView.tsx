import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import {
  A4_ASPECT,
  SHEET_GAP,
  pageRows,
  rasterWidth,
  resolveSpread,
  rowStartOf,
  sheetSize,
  sheetUrl,
  type PdfFit,
  type PdfResolvedSpread,
  type PdfSpread,
} from "./pdfPaging";

/**
 * PDF 阅读面：**一条纵向连续的纸带**（SumatraPDF 式）。
 *
 * 这里是父页里的普通 DOM——**没有 iframe、没有分栏分页器、没有页窗口**。一「行」放
 * 一张或两张纸（封面单独，之后 2-3 / 4-5…），缩放随窗口实时重算，滚动就是滚动。
 * 版式数学全在 `pdfPaging.ts`（纯函数，可单独跑断言），本文件只负责：量阅读区、
 * 铺占位、拼图片 URL、把滚动与按键翻译成页号与位置。
 *
 * 几个刻意的选择：
 *
 * - **整条纸带都有占位尺寸**（Rust 的 `pageSizes` 给真实页面比例）→ 滚动条长度与
 *   「跳到第 N 页」从第一帧起就准确，不必等图片加载。缺 `pageSizes` 时先按 A4 兜底，
 *   图片加载后按实测比例修正（只在缺项时测）。
 * - **纸的尺寸走 CSS 变量**（`.pdf-sheet` 读 `--pdf-sheet-w/h`）：窗口拖动时只改纸带
 *   上那两个变量，952 行不必带着新 style 对象重渲染；比例与众不同的页才在自己身上
 *   写死尺寸。
 * - **图片懒加载**用 `loading="lazy"`：占位全在（滚动条准），但只请求视口附近那几张
 *   ——952 页的书不会一次发 952 个请求。Rust 侧另有整段预取，翻页几乎即时。
 * - **当前页**＝视口里可见面积最大的那张；并排两张可见高相同，于是**左页**胜出
 *   （面积＝可见高 × 纸宽），规则确定、不抖。
 * - 换缩放/单双页/窗口尺寸时**把当前页保持在视口顶部**（SumatraPDF 行为）；用户滚动
 *   时绝不反向拽位置——只有显式跳页与「布局定下来」这两个时机才写 `scrollTop`。
 */

/** 行距与中缝同值，视觉一致；[`SHEET_GAP`] 也是并排判据用的值。 */
const ROW_GAP = SHEET_GAP;
/** 纸带上下留白（模型里也算进去，保证 scrollTop 与 DOM 完全对得上）。 */
const STRIP_PAD = 12;
/** 窗口尺寸停下来多久才换 `?w=`（布局立刻跟，只有栅格请求等它）。 */
const RASTER_DEBOUNCE_MS = 150;
/** 比例相差超过这个值才算「与众不同的页」，值得写死尺寸。 */
const ASPECT_EPSILON = 0.005;

export type PdfViewHandle = {
  /** 上一页 / 下一页（双页按跨页）。 */
  goPage: (delta: -1 | 1) => void;
  /** 跳页：把该页所在的行滚到视口顶部；`fraction`（0–1）用于页内定位。 */
  goToPage: (page: number, fraction?: number) => void;
  /** PageDown / Space：滚一屏。 */
  scrollScreen: (delta: -1 | 1) => void;
};

export type PdfViewState = {
  /** 视口里面积最大的那张纸（1-based）。 */
  page: number;
  /** 实际张数（「自动」判定之后）。 */
  spread: PdfResolvedSpread;
  atStart: boolean;
  atEnd: boolean;
};

type Props = {
  bookId: string;
  resourceOrigin: string;
  pageCount: number;
  /** Rust 的 `OpenedBook.pageSizes`：`[宽, 高]`（PDF 单位，只用比例）。 */
  pageSizes?: [number, number][];
  fit: PdfFit;
  spread: PdfSpread;
  /** 打开书时的落点（1-based，来自进度）。 */
  initialPage?: number;
  onState?: (state: PdfViewState) => void;
};

/** 缺 `pageSizes` 时也不会每次渲染都换数组身份。 */
const NO_SIZES: [number, number][] = [];

const clampPage = (page: number, pageCount: number) =>
  Math.min(Math.max(1, Math.round(page)), Math.max(1, pageCount));

const PdfView = forwardRef<PdfViewHandle, Props>(function PdfView(
  {
    bookId,
    resourceOrigin,
    pageCount,
    pageSizes = NO_SIZES,
    fit,
    spread,
    initialPage = 1,
    onState,
  },
  ref,
) {
  const scrollRef = useRef<HTMLDivElement>(null);
  /** 阅读区尺寸：布局立刻跟（拖窗口时纸跟着变）。 */
  const [area, setArea] = useState({ width: 0, height: 0 });
  /** 防抖后的尺寸：只用来拼 `?w=`，避免每个像素都换一次栅格请求。 */
  const [rasterArea, setRasterArea] = useState({ width: 0, height: 0 });
  /** 缺 `pageSizes` 时用图片实测补上的页面比例（page → 宽/高）。 */
  const [measured, setMeasured] = useState<Record<number, number>>({});
  const [page, setPage] = useState(() => clampPage(initialPage, pageCount));
  const pageRef = useRef(page);
  pageRef.current = page;
  const onStateRef = useRef(onState);
  onStateRef.current = onState;
  const scrollRaf = useRef<number | null>(null);
  const rasterReady = useRef(false);
  const dpr = window.devicePixelRatio || 1;

  /** 页面比例：Rust 的 `pageSizes` 优先，其次图片实测，最后 A4 兜底。 */
  const aspectOf = useCallback(
    (target: number): number => {
      const size = pageSizes[target - 1];
      if (size && size[1] > 0 && size[0] > 0) return size[0] / size[1];
      const local = measured[target];
      return local && local > 0 ? local : A4_ASPECT;
    },
    [pageSizes, measured],
  );

  /** 实际张数（「自动」按当前页真实比例算）。 */
  const resolved = useMemo(
    () => resolveSpread(spread, aspectOf(page), area.width, area.height),
    [spread, aspectOf, page, area.width, area.height],
  );

  const areaW = area.width > 0 ? area.width : 1;
  const areaH = area.height > 0 ? area.height : 1;

  /** 当前页那张纸的尺寸——纸带的 CSS 变量就用它。 */
  const defaultSize = useMemo(
    () => sheetSize(aspectOf(page), fit, resolved, areaW, areaH),
    [aspectOf, page, fit, resolved, areaW, areaH],
  );

  /**
   * 整条纸带的几何：每行高度（取行内较高的一张）与前缀和。行高由页面比例直接算出，
   * 与 DOM 一一对应（行距与上下留白都算进去），所以跳页精确且不必读 DOM。
   */
  const layout = useMemo(() => {
    const rows = pageRows(pageCount, resolved);
    const heights: number[] = [];
    for (const row of rows) {
      let height = 1;
      for (const target of row) {
        height = Math.max(
          height,
          sheetSize(aspectOf(target), fit, resolved, areaW, areaH).height,
        );
      }
      heights.push(height);
    }
    const tops: number[] = [];
    let acc = STRIP_PAD;
    for (const height of heights) {
      tops.push(acc);
      acc += height + ROW_GAP;
    }
    const rowIndexOfStart = new Map<number, number>();
    rows.forEach((row, index) => rowIndexOfStart.set(row[0], index));
    return { rows, heights, tops, rowIndexOfStart };
  }, [pageCount, resolved, aspectOf, fit, areaW, areaH]);

  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const resolvedRef = useRef(resolved);
  resolvedRef.current = resolved;
  const aspectRef = useRef(aspectOf);
  aspectRef.current = aspectOf;

  const rowIndexForPage = useCallback(
    (target: number): number =>
      layout.rowIndexOfStart.get(rowStartOf(target, resolved)) ?? 0,
    [layout, resolved],
  );

  /** 视口里可见面积最大的那张纸。并排两张可见高相同 → 左页（先出现）胜出。 */
  const pageAtScroll = useCallback(
    (scrollTop: number, viewportHeight: number): number => {
      const l = layoutRef.current;
      if (l.rows.length === 0) return 1;
      let lo = 0;
      let hi = l.tops.length - 1;
      let first = 0;
      while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        if (l.tops[mid] <= scrollTop) {
          first = mid;
          lo = mid + 1;
        } else {
          hi = mid - 1;
        }
      }
      const bottom = scrollTop + viewportHeight;
      const mode = resolvedRef.current;
      let bestPage = l.rows[first][0];
      let bestArea = -1;
      for (let index = first; index < l.rows.length; index += 1) {
        const top = l.tops[index];
        if (top >= bottom) break; // 之后的整行都在视口下方
        for (const target of l.rows[index]) {
          const size = sheetSize(
            aspectRef.current(target),
            fit,
            mode,
            areaW,
            areaH,
          );
          const visible = Math.max(
            0,
            Math.min(top + size.height, bottom) - Math.max(top, scrollTop),
          );
          const sheetArea = visible * size.width;
          if (sheetArea > bestArea) {
            bestArea = sheetArea;
            bestPage = target;
          }
        }
      }
      return bestPage;
    },
    [fit, areaW, areaH],
  );

  /** 滚动 → 当前页（rAF 节流；只上报变化，不写 scrollTop）。 */
  const handleScroll = useCallback(() => {
    if (scrollRaf.current !== null) return;
    scrollRaf.current = requestAnimationFrame(() => {
      scrollRaf.current = null;
      const box = scrollRef.current;
      if (!box) return;
      const next = pageAtScroll(box.scrollTop, box.clientHeight);
      if (next !== pageRef.current) {
        pageRef.current = next;
        setPage(next);
      }
    });
  }, [pageAtScroll]);

  /** 把某页所在的行滚到视口顶部（只有跳页与布局定下来才走这里）。 */
  const scrollToPage = useCallback(
    (target: number, fraction = 0) => {
      const box = scrollRef.current;
      const l = layoutRef.current;
      if (!box || l.rows.length === 0) return;
      const wanted = clampPage(target, pageCount);
      const index = rowIndexForPage(wanted);
      const top = l.tops[index] ?? STRIP_PAD;
      const offset = fraction > 0 ? fraction * (l.heights[index] ?? 0) : 0;
      box.scrollTop = Math.max(0, top + offset);
      if (wanted !== pageRef.current) {
        pageRef.current = wanted;
        setPage(wanted);
      }
    },
    [pageCount, rowIndexForPage],
  );

  useImperativeHandle(
    ref,
    () => ({
      goPage: (delta: -1 | 1) => {
        const l = layoutRef.current;
        if (l.rows.length === 0) return;
        const index = rowIndexForPage(pageRef.current) + delta;
        if (index < 0 || index >= l.rows.length) return;
        scrollToPage(l.rows[index][0]);
      },
      goToPage: (target: number, fraction = 0) => scrollToPage(target, fraction),
      scrollScreen: (delta: -1 | 1) => {
        const box = scrollRef.current;
        if (!box) return;
        box.scrollTop = Math.max(
          0,
          box.scrollTop + delta * Math.max(80, box.clientHeight * 0.9),
        );
      },
    }),
    [rowIndexForPage, scrollToPage],
  );

  // 量阅读区：ResizeObserver（窗口/全屏/分栏变化都跟着走）。
  useEffect(() => {
    const box = scrollRef.current;
    if (!box) return;
    const measure = () => {
      const width = box.clientWidth;
      const height = box.clientHeight;
      setArea((prev) =>
        prev.width === width && prev.height === height
          ? prev
          : { width, height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, []);

  // 栅格尺寸：首帧立刻采用，之后防抖 150ms（同档不换 URL 由量化保证）。
  useEffect(() => {
    if (!rasterReady.current && area.width > 0) {
      rasterReady.current = true;
      setRasterArea(area);
      return;
    }
    const timer = window.setTimeout(() => setRasterArea(area), RASTER_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [area]);

  // 布局定下来（换缩放 / 换单双页 / 窗口尺寸变了）→ 当前页保持在视口顶部。
  // 依赖用的是**防抖后**的尺寸：拖窗口途中不反复拽位置，停下来才对齐一次。
  useEffect(() => {
    const box = scrollRef.current;
    const l = layoutRef.current;
    if (!box || l.rows.length === 0 || area.width <= 0) return;
    const index =
      l.rowIndexOfStart.get(rowStartOf(pageRef.current, resolved)) ?? 0;
    box.scrollTop = Math.max(0, l.tops[index] ?? STRIP_PAD);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fit, resolved, rasterArea.width, rasterArea.height, bookId]);

  // 换书：同一个组件实例被复用时回到新的落点。
  useEffect(() => {
    const wanted = clampPage(initialPage, pageCount);
    pageRef.current = wanted;
    setPage(wanted);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId]);

  // 上报给壳：页码 + 实际张数 + 是否到头（顶栏禁用态与进度都靠它）。
  useEffect(() => {
    if (layout.rows.length === 0) return;
    const index = rowIndexForPage(page);
    onStateRef.current?.({
      page,
      spread: resolved,
      atStart: index <= 0,
      atEnd: index >= layout.rows.length - 1,
    });
  }, [page, resolved, layout, rowIndexForPage]);

  useEffect(() => {
    return () => {
      if (scrollRaf.current !== null) cancelAnimationFrame(scrollRaf.current);
    };
  }, []);

  /** 缺 `pageSizes` 时按实测比例修正占位（只在缺项时测）。 */
  const handleSheetLoad = useCallback(
    (target: number, img: HTMLImageElement) => {
      if (pageSizes[target - 1]) return;
      if (!img.naturalWidth || !img.naturalHeight) return;
      const value = img.naturalWidth / img.naturalHeight;
      setMeasured((prev) =>
        prev[target] !== undefined && Math.abs(prev[target] - value) < 0.02
          ? prev
          : { ...prev, [target]: value },
      );
    },
    [pageSizes],
  );

  /** 一张纸该请求多宽的栅格：按它**实际画多大**（防抖后的尺寸），不是容器宽。 */
  const rasterFor = useCallback(
    (target: number): number => {
      const source = rasterArea.width > 0 ? rasterArea : area;
      const size = sheetSize(
        aspectOf(target),
        fit,
        resolved,
        source.width > 0 ? source.width : 1,
        source.height > 0 ? source.height : 1,
      );
      return rasterWidth(size.width, dpr);
    },
    [rasterArea, area, aspectOf, fit, resolved, dpr],
  );

  const stripStyle = {
    paddingTop: STRIP_PAD,
    paddingBottom: STRIP_PAD,
    rowGap: ROW_GAP,
    "--pdf-sheet-w": `${defaultSize.width}px`,
    "--pdf-sheet-h": `${defaultSize.height}px`,
  } as CSSProperties;

  return (
    <div className="pdf-scroll" ref={scrollRef} onScroll={handleScroll}>
      <div className="pdf-strip" style={stripStyle}>
        {layout.rows.map((row) => (
          <div className="pdf-row" key={row[0]}>
            {row.map((target) => {
              const aspect = aspectOf(target);
              // 比例与「当前页」不同的纸才写死尺寸；其余读纸带上的 CSS 变量，
              // 这样拖窗口时不必让每一行都带着新样式重渲染。
              const odd = Math.abs(aspect - defaultSize.width / defaultSize.height) >
                ASPECT_EPSILON;
              const size = odd
                ? sheetSize(aspect, fit, resolved, areaW, areaH)
                : null;
              return (
                <div
                  className="pdf-sheet"
                  key={target}
                  data-page={target}
                  style={
                    size ? { width: size.width, height: size.height } : undefined
                  }
                >
                  {/* 懒加载：占位全在（滚动条准），但只请求视口附近那几张。
                      `resourceOrigin` 还没到（IPC 首帧）时不发 src——否则会按应用
                      自身地址发一串 404；它一到就自动补上。 */}
                  {resourceOrigin && (
                    <img
                      src={sheetUrl(
                        resourceOrigin,
                        bookId,
                        target,
                        rasterFor(target),
                      )}
                      alt={`第 ${target} 页`}
                      loading="lazy"
                      decoding="async"
                      draggable={false}
                      onLoad={(event) =>
                        handleSheetLoad(target, event.currentTarget)
                      }
                    />
                  )}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
});

export default PdfView;
