// PDF 纸带模型的自检（仓库没有前端测试 runner，用 node 直接跑这个模块）。
// Run: node ui/src/pdfPaging.check.ts   （Node 22.6+ 直接剥离类型）
import {
  A4_ASPECT,
  SHEET_GAP,
  autoSpread,
  pageRows,
  rasterWidth,
  resolveSpread,
  rowStartOf,
  sheetSize,
  sheetUrl,
} from "./pdfPaging.ts";

let failures = 0;
const eq = (label: string, actual: unknown, expected: unknown) => {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) {
    failures += 1;
    console.log(`FAIL ${label}: got ${a}, want ${e}`);
  } else {
    console.log(`ok   ${label} = ${a}`);
  }
};

// 真实页面比例：A4 竖版 ≈ 210/297；横向 A4 是其倒数（≈1.414）；封面常见 2:3。
const A4 = A4_ASPECT;
const LANDSCAPE = 297 / 210;
const COVER = 2 / 3;

// ---- 配对：封面单独，之后 2-3 / 4-5；单页一行一张 ----
eq("rows double 5p", pageRows(5, "double"), [[1], [2, 3], [4, 5]]);
eq("rows double 4p", pageRows(4, "double"), [[1], [2, 3], [4]]);
eq("rows double 1p", pageRows(1, "double"), [[1]]);
eq("rows double 0p", pageRows(0, "double"), []);
eq("rows single 3p", pageRows(3, "single"), [[1], [2], [3]]);
eq("row start 1", rowStartOf(1, "double"), 1);
eq("row start 2", rowStartOf(2, "double"), 2);
eq("row start 3 → 2", rowStartOf(3, "double"), 2);
eq("row start 5 → 4", rowStartOf(5, "double"), 4);
eq("row start single 3", rowStartOf(3, "single"), 3);

// ---- 自动双页：按真实页面比例判（两种缩放共用同一条判据）----
eq("auto A4 in 1152x780", autoSpread(A4, 1152, 780), "double");
eq("auto A4 in 1600x900", autoSpread(A4, 1600, 900), "double");
eq("auto cover 2:3 in 1600x900", autoSpread(COVER, 1600, 900), "double");
// 横向页：两页按高度档算出来的宽度塞不进 → 单页（窗口再宽也一样）。
eq("auto landscape in 1600x900", autoSpread(LANDSCAPE, 1600, 900), "single");
eq("auto landscape in 2560x1440", autoSpread(LANDSCAPE, 2560, 1440), "single");
// 窄窗 / 竖屏窗口：一张纸铺满都紧张，别谈并排。
eq("auto A4 in 800x1200 (portrait)", autoSpread(A4, 800, 1200), "single");
eq("auto A4 in 700x780 (narrow)", autoSpread(A4, 700, 780), "single");
eq("auto A4 in 900x900 (1 页就占满宽)", autoSpread(A4, 900, 900), "single");
eq("auto unknown aspect", autoSpread(0, 1600, 900), "single");
// 手动项一律照办（横向页也允许手动双页）。
eq("resolve manual double wins", resolveSpread("double", LANDSCAPE, 900, 900), "double");
eq("resolve manual single wins", resolveSpread("single", A4, 1600, 900), "single");
eq("resolve auto delegates", resolveSpread("auto", A4, 1600, 900), "double");

// ---- 一张纸画多大 ----
// 适应宽度：纸宽 = 阅读区宽（双页各占一半），高按比例。
eq("size fit-width single 1600x900 w", Math.round(sheetSize(A4, "width", "single", 1600, 900).width), 1600);
eq("size fit-width single 1600x900 h", Math.round(sheetSize(A4, "width", "single", 1600, 900).height), 2263);
eq("size fit-width double 1600x900 w", Math.round(sheetSize(A4, "width", "double", 1600, 900).width), 788);
// 适应页面：纸高 = 阅读区高，宽按比例。
eq("size fit-page single 1600x900 h", Math.round(sheetSize(A4, "page", "single", 1600, 900).height), 900);
eq("size fit-page single 1600x900 w", Math.round(sheetSize(A4, "page", "single", 1600, 900).width), 636);
eq("size fit-page landscape 1600x900 w", Math.round(sheetSize(LANDSCAPE, "page", "single", 1600, 900).width), 1273);
// 适应页面双页：放得下按高度；放不下整体缩小到放得下（不横向裁掉），比例不变。
const fits = sheetSize(A4, "page", "double", 1600, 900);
eq("size fit-page double fits h", Math.round(fits.height), 900);
const tight = sheetSize(A4, "page", "double", 900, 900);
eq("size fit-page double tight: 2w+gap<=area", 2 * tight.width + SHEET_GAP <= 900 + 1e-6, true);
eq("size fit-page double tight: aspect kept", Math.round((tight.width / tight.height) * 1000) / 1000, Math.round(A4 * 1000) / 1000);
// 未知比例用 A4 兜底。
eq("size unknown aspect → A4", Math.round(sheetSize(0, "page", "single", 1600, 900).width), Math.round(A4 * 900));

// ---- 栅格宽度：按纸实际画多大算，不是容器多宽 ----
eq("raster A4 fit-page 1600x900 @2", rasterWidth(sheetSize(A4, "page", "single", 1600, 900).width, 2), 1320);
eq("raster landscape fit-page 1600x900 @2", rasterWidth(sheetSize(LANDSCAPE, "page", "single", 1600, 900).width, 2), 2400);
eq("raster A4 fit-width 1600x900 @1", rasterWidth(sheetSize(A4, "width", "single", 1600, 900).width, 1), 1560);
eq("raster tiny → min", rasterWidth(200, 1), 720);
eq("raster huge → max", rasterWidth(4000, 1), 2400);
eq("raster unknown dpr", rasterWidth(600, 0), 720);

// ---- 图片 URL 契约（Rust: page/NNNN.webp?w=）----
eq("url basic", sheetUrl("http://x", "ABC", 7, 900), "http://x/book/ABC/page/0007.webp?w=900");
eq("url pad", sheetUrl("http://x", "ABC", 1234, 1200), "http://x/book/ABC/page/1234.webp?w=1200");
eq("url rounds width", sheetUrl("http://x", "ABC", 1, 1199.6), "http://x/book/ABC/page/0001.webp?w=1200");

if (failures > 0) throw new Error(`${failures} paging check failures`);
console.log("ALL OK");
