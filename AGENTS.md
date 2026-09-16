# Agent 说明

给在本仓库改代码的人（含编码助手）用。产品说明见 `README.md`。这里只留禁则和入口；实现契约写在对应模块注释里，不要把同一条事实再抄一遍。

## 这是什么

IcedReader 是 Windows 桌面电子书阅读器（不是 Iced GUI）。栈：Tauri 2 + React + Rust。只做 Windows；业务不要写进 Windows 专用 API（Tauri/WebView 边界留在 `src-tauri`）。

## 分层（不要打穿）

| 路径 | 可以做 | 不要做 |
| --- | --- | --- |
| `crates/core` | `Book` / `Locator` / 进度存储 | zip、OPF、PDF、Tauri、DOM |
| `crates/formats-epub` | 打开 EPUB、出 HTML 和资源 | UI、SQLite、直接给前端 rbook 类型 |
| `crates/formats-pdf` | 打开 PDF、栅格化页面、出页 HTML 和图片资源、字体与质量体检 | UI、排版皮肤、把 hayro/lopdf 类型外泄到前端 |
| `src-tauri` | 命令、文件对话框、`icedreader` 协议、便携目录、书库扫描（经 `BookOpener` 分派） | 自己 unzip/解析 OPF、直接调 hayro/lopdf、排版 |
| `ui` | 阅读壳、交互 | 读磁盘上的 epub/pdf 字节 |

格式层按 `Book` + `BookOpener`（`crates/formats-epub`、`crates/formats-pdf`；分派只有一处：`src-tauri/src/openers.rs`）。解析/渲染不进 JS；前端命令形状尽量不动。新增格式 = 新 crate + `openers()` 一行，`src-tauri`/`ui` 只按 `format` 分支。

## 硬约束

1. **正文是 HTML。** 资源 URL 在 Rust 里改写成 `http://icedreader.localhost/book/{id}/...`，前端用 `srcDoc` 显示。**禁止往章节注入阅读皮肤**（颜色、装饰性字号、字体族、行距、`max-width` 居中、主题）。禁止黑色主题、禁止主题切换、禁止用 `prefers-color-scheme` 把壳或正文改暗。父页只允许这些注入（合法内容以模块注释为准）：
   - 四槽字体齐且关掉「使用原书字体」：`@font-face` + 改写 `font-family`（`crates/core/src/fonts.rs`、`settings.rs`）
   - `#iced-reader-flow` 栏式分页（`ui/src/flowLayout.ts`）
   - `#iced-reader-highlight-style` 用户划线（`ui/src/highlights.ts`）
   - `#iced-reader-note-style` 词注呈现（`ui/src/wordNotes.ts`；展开在 `crates/formats-epub/src/footnotes.rs`）
   - `#iced-reader-cover-fit` 近空章整页背景封面（`ui/src/coverFit.ts`）
2. **进度只存 `Locator`：** `href` + `fraction`（0～1）+ 可选 `cfi`。禁止 `scrollTop`。`cfi` 留空，不要填假值。
3. **进度键：** 有 identifier 用 `id:...`；否则 `lib:...`（相对便携书库）。`path:` 仅无书库目录时回退。外部 EPUB/PDF 按文件名进 `data/library/`，同名复用、不另存 `-2`。`lib:书名-N.epub` 与 `lib:书名.epub` 视为同一本；**扩展名是身份的一部分**，同名 `.epub` 与 `.pdf` 是两本书（改名/删书不得互相牵连，PDF 一期不产 identifier）。见 `progress_key` / `same_book` / `book_stem`。
4. **章节 iframe 只要 `allow-same-origin`，不要 `allow-scripts`。** EPUB 内容不开脚本。书内链接拦在父页，iframe 必须保持 `about:srcdoc`。
5. **IPC camelCase**（`#[serde(rename_all = "camelCase")]`）。
6. **绿色软件：** 状态一律 `{exe}/data/`，禁止 `%APPDATA%` / 注册表当主存储，不要用会写用户目录的 window-state 插件。目录必须可写，不要装进 Program Files。文件布局见 `src-tauri/src/portable.rs`。

## 常用命令

```powershell
npm install
.\scripts\dev.ps1
cargo test -p iced-reader-core
cargo test -p iced-reader-epub
cargo test -p iced-reader-pdf
cargo test -p iced-reader
npx tsc --noEmit
```

PDF 样本自检（不入库，放哪都行）：`cargo run -p iced-reader-pdf --example render -- <file.pdf|目录> [--pages 1-3] [--dump-page N] [--format png|webp]` —— 体检字体嵌入、页面算子/文字层、栅格/编码耗时，`--pages` 时把图写到 `<书名>.spike/`（已 gitignore）。另有 `--example svg_probe -- <file.pdf> [page]`（dev-dependency，判断"矢量 SVG 能否替代栅格"，实测结论见 `docs/ideas/pdf.md`）。

自动打开样书：`$env:ICED_READER_OPEN = "$PWD\fixtures\sample.epub"`。Windows 编译需要 MSVC；`scripts/dev.ps1` 会载入 vsvars，打 release 前同样要载入。

用户说 build release 时：`npm run tauri -- build --no-bundle`，再用 `Copy-Item`（禁止 Rename-Item / 硬链接）拷到 `target/release/IcedReader-{version}-windows-x64.exe`，`{version}` 取 `src-tauri/tauri.conf.json`。告诉用户这份路径，GitHub Release 上传它。不要把 Cargo 原始产物 `target/release/IcedReader.exe`（或 `deps/` 下中间 exe）当发布文件。只构建绿色版（`--no-bundle`），不要跑不带该旗标的 `build`。不要用 `--offline` 除非依赖已在本地。

## Tauri 命令

形状以代码为准。前端必须先确认再调 `delete_book`。

- 书：`open_book` / `close_book` / `pending_book` / `list_library` / `delete_book`
- 元数据：`get_book_meta` / `set_book_meta` / `reread_book_meta`
- 阅读：`get_chapter`（`{ html, publisherFonts }`）/ `resource_origin` / `save_progress`
- 划线/备注：`list_annotations` / `add_annotation` / `delete_annotation` / `save_note` / `read_notes`
- 字体：`get_font_settings` / `set_use_original_fonts` / `set_font_scale` / `install_font` / `clear_font`

## 书元数据

伴生 `data/library/<stem>.md`，**程序维护**，用户只走书架「编辑元数据…」面板。字段、拼接、裁决链、改名见 `crates/core/src/book_meta.rs`。保存后按显示名改库内 epub+md+notes.md；`lib:` 键迁进度/划线/质量缓存；改名失败则整次保存报错。`delete_book` 连带删 md。程序生成的分隔符一律 ASCII，不产出全角；原书 `dc:title` 自带字符原样保留。

## EPUB 章节

- **目录锚点当章：** TOC ≥ 2 且（任一 href 带 `#` 或 TOC 树条目数 > OPF spine）时用摊平 TOC（href 保留 `#`），`chapter_html` 按锚点切到下一 TOC 锚点。少于两条不摊平。正规一章一文件仍走 OPF spine。见 `crates/formats-epub/src/lib.rs`。
- **路径改写**要能过不规范 HTML：rbook XML 失败后宽松改写相对 `src`/`href`，不要为此给章节开脚本。
- **`lang`：** `srcDoc` 无 `lang` 时从 `xml:lang` 或 `dc:language` 补（跳过 `und`）。不要为此灌皮肤。
- **分页：** CSS 多栏，栏宽上限 720px、最多 2 栏，竖屏 1 栏；多出的宽度是页边。iframe 按页数拉宽，外层 `scrollLeft` 翻页。左右键 / 滚轮翻页（无点左右侧翻页）；章边界续翻。进度仍是 `href` + 章内 `fraction`。

## PDF（第一期：只读）

- **一页 = 一个 spine 单元**：`href = page/0001`（1 起、4 位补零），`title` 取最近的 outline 条目，`toc` 直接用 PDF outline（**无 outline 就不给目录**，不做页序长列表）。`chapterChars` = 每页 1，于是按位置跳转天然按页工作。见 `crates/formats-pdf/src/book.rs`。
- **版式 = SumatraPDF 式连续纸带**（用户 2026-09-15 拍板「PDF 不是 EPUB」）：PDF **不走** EPUB 的分栏分页器。`ui/src/PdfView.tsx` 在**父页里**渲染一条纵向连续的纸带（`.pdf-scroll/.pdf-strip/.pdf-row/.pdf-sheet`，普通 DOM：没有 iframe、没有 `#iced-reader-flow`、不调 `get_chapter`），一「行」放 1 张或 2 张纸，配对是书式（封面单独、之后 2-3 / 4-5）。
  - **缩放随窗口实时重算**（ResizeObserver，不是一次性的 resize 监听）：**适应宽度 = 纸宽动态贴合阅读区宽、恒单页**（要并排就用「适应页面 + 双页/自动」）；**适应页面 = 纸高贴合窗口高**；双页的「自动」按**实际页面宽高比**判断两页能否并排（横向页自然判单页）。进度区在并排时显示「· 跨页」。
  - 滚轮是原生滚动，←/→ 翻页（双页按跨页），PageDown/空格滚一屏，Home/End 首末页；目录/跳页 = 把目标页滚到视口顶部。**跳转收页号**：进度区的「· 跳页」输入第 N 页（越界夹到首/末页，非数字不跳）；PDF 各页等重，全书 % 没有额外信息，那个入口只留给 EPUB。**「当前页」= 视口面积最大的那张**，它决定顶栏「第 N / M 页」与保存的进度（`href = page/NNNN`、`fraction = 0`）。
- **渲染在 Rust、版式在前端**：`hayro` 把页面栅格化成无损 WebP，`lopdf` 读结构（页数/Info/outline——`hayro-syntax` 不暴露 outline）。前端用 `OpenedBook.pageSizes`（= `Book::page_sizes()`；PDF 给全部页尺寸，EPUB 为空）**先铺出整条纸带的占位**——滚动条长度与「跳到第 N 页」从第一帧就准确——图片 `loading="lazy"`，URL 直接拼 `{origin}/book/{id}/page/0007.webp?w=`。`chapter_html` 只剩别的调用方在用，壳不再为 PDF 调 `get_chapter`。
- **缓存与预取**：`resource` 每次请求**一页**（页窗口机制已随旧版式删除），请求会排队它左右各 2 页预取（实测命中后相邻页 0.1–0.6 ms，冷页 18–143 ms）。内存 LRU 16 页；**不要做全书磁盘缓存**；**不要做开书预热**（宽度未知，猜出来的宽度随后会被重新渲染）。栅格宽度由前端按**纸实际显示多大** × DPR 算出、量化后传 `?w=`，Rust 再按 `WIDTH_STEP`(128px) 分桶——邻近宽度共用一个缓存项，窗口缩放不会把缓存打碎。
- **编码用无损 WebP**（页面与封面，2026-09-15 实测）：1440px 文字页 PNG 15.6 ms / 498 KB → WebP 20.3 ms / 224 KB，扫描页 PNG 19.8 ms / 1042 KB → WebP 27.5 ms / 324 KB。多花 5–8 ms 编码（在预取线程里、不占翻页路径）换 **55–69% 的字节**与同比例的缓存内存；封面 400px 也小 16–45%。**JPEG 绝不要用**：编码 65–95 ms（比 WebP 慢 3–4 倍）且字节还更多。hayro-syntax 的 `unsafe` 特性实测无收益（98.1/45.3/23.3 ms vs 开启后 110.1/46.4/28.2 ms），已关。扫描页的耗时大头是 hayro 解码内嵌图像（约 40–60 ms 固定成本），降 `w` 是唯一有效手段。
- **封面**：PDF 没有 cover 资源，`library::cover_bytes` 用第 1 页 400px 渲染顶上（`BookProfile.has_cover = true`）。
- **字体：不做替换（用户 2026-09-15 拍板）**。PDF 的字形由文件本身定死（栅格化后已进 PNG），阅读器四槽字体对 PDF 无意义（顶栏已隐藏字号/字体）。hayro 的 `font_resolver` 确实能救「未嵌入 + 有 ToUnicode」的字体（实测：默认画成拉丁垃圾 `A Æ Á`，喂 `simsun.ttc` 后正确画出 `一 二 三`），但**不做成设置项**；`PdfDoc::render_page_png_with` + `--substitute-font` 只作为诊断入口留着（裸 `Identity-H` 无 ToUnicode 时谁也救不了）。
- **质量角标**：PDF **有自己的信号与评分**（`crates/formats-pdf/src/quality.rs` + `book_signals::{analyze_pdf, grade_pdf}`），按「读者能拿它做什么」判定（用户 2026-09-15 口径）：真文字 + 字体全嵌入 + 有书签目录（且无缺字风险、有书名作者）= **优**；扫描 + OCR 文字层（可搜）= **良**；纯扫描无可提取文字 = **中**；打不开/加密在书架显示「无法打开」。EPUB 仍走 `grade()`，两套评分互不通用；`list_library` 只读缓存，不在热路径重算。`book_signals::BookSignals.pdf` 为空 = 还没算过（PDF）/ 不是 PDF（EPUB）。
- **同书提示**：PDF 用「页数 + 书名 + 目录标签 + producer」的粗指纹（EPUB 用正文字符指纹）。**空指纹不得参与同书分组**（`library.rs` 已过滤），否则所有缺指纹的书会被当成同一本。
- **格式特有的书内提示**：`open_book` 返回 `warnings`。PDF 用 `iced_reader_pdf::visible_text_risk`（抽样若干页做算子普查）判断「**可见**文字用了未嵌入的非标准字体」——只有这种会缺字；`Tr=3` 隐藏 OCR 文字层与纯扫描页无害（实测：扫描书的 14 个「未嵌入字体」全在隐藏层，画面完全正常）。判据别退回成「字体没嵌入就报警」。
- **一期不做**：文字选择、划线/备注、搜索、任意百分比缩放、加密 PDF（hayro 不支持解密，打开报错）。二期从 **PDF 自己的文字算子**生成定位文字层（真文字书与「扫描 + OCR 文字层」都覆盖），纯扫描书只能做区域划线。设计与实测数字见 `docs/ideas/pdf.md`。
- 样本自检见「常用命令」；`*.spike/` 是渲染出的对照 PNG，已 gitignore。

## 改 UI 时

- 阅读区铺满顶栏以下。正文用 `.flow-host` 栅格居中，不要往章节 HTML 里注入 `max-width` / `margin: auto`。
- 按 `book.format` 分支（`"epub"` / `"pdf"`）：**PDF 走 `ui/src/PdfView.tsx`**（连续纸带），隐藏字号 A± 与字体面板、隐藏划线入口、进度区显示「第 N / M 页」（并排时加「· 跨页」）、显示 `warnings` 提示条；**EPUB 继续走 `ChapterFrame`**（它现在对 PDF 一无所知，别把 PDF 逻辑加回去），行为一字不许变。
- 字体面板：原书 CSS 声明 vs 本章实际绘制，规则见 `ui/src/usedFonts.ts`。在注入自定义字体之前从原 HTML/CSS 抽取；`src` 不在书内的 `@font-face` 标「书内无字体文件」。
- 目录用 `book.toc`（否则 spine 标题）。不要在前端 parse NCX。
- 书架：`ui/src/Library.tsx`。封面走 `/library-cover/{文件名}` + `coverRev`，不要 `immutable` 长缓存，不要在 JS 里 unzip / 把封面字节塞进 `list_library`。回书架先 `await` 进度再 `list_library`。封面三点菜单（悬停才显示）：「编辑元数据…」在上、「从书库删除」在下（先确认）。统计行、质量角标、同书提示由 `list_library` 字段纯前端算；`list_library` 不在热路径重算质量。书库只扫 `data/library/` 一层（`*.epub` / `*.pdf`）。开发 `target/debug/data` 与 release `target/release/data` 是两套。
- 顶栏 `.chrome` 固定 52px、不换行；窄窗口（≤1180px）把低频按钮收进右上「⋯」。不放品牌字。全屏用 Tauri `setFullscreen`（F11），关掉 WebView2 浏览器加速键。Esc：先关浮层/目录，再退出全屏。全屏顶栏默认收起，窗口顶部整条热区可唤出。位置/大小/最大化存 `data/window.json`；全屏不当下次启动状态。不要在 `CloseRequested` 里 `prevent_close`。
- 界面文案默认中文。提交信息用中文，说明做了什么、为什么。

## 不要做

- 不要把解析逻辑搬进 JS（包括让 foliate-js 直接 unzip、让 pdf.js 直接读文件）。Rust 负责打开与栅格化，前端只显示结果。
- 不要做黑色主题、暗色模式、主题切换。本软件就是浅色纸。
- 不要提交 `target/`、`node_modules/`、`ui/dist/`、`src-tauri/gen/`、`data/`、`fixtures/verify-*.png`、`*.spike/`、仓库根目录的本地 EPUB/PDF（`fixtures/sample.epub` 除外）。
- 不要在文档或代码里写用户的密钥、本机绝对路径（样例用仓库相对路径）。

## 验证

改哪测哪。没有桌面窗口时至少跑 `cargo test -p iced-reader-core`、`iced-reader-epub`、`iced-reader-pdf`、`iced-reader`。样书：`fixtures/sample.epub`；仓库根目录未提交的 `經濟漩渦.pdf` / `Windows Everywhere - Paul Thurrott.pdf` / `语文开窍(带目录)….pdf` 以及旧的 `资治通鉴.epub` / `五千年掌故.epub` / `新西游记++共两册.epub` 不要 git add。

有桌面窗口时用 `scripts/ui-probe.ps1` 驱动真实窗口：启动绿色版、抢前台置顶、发真键鼠（SendKeys，方向键写 `{RIGHT}`）、把客户区截图。配 `scripts/crop.ps1`（放大局部看小字）、`scripts/imgdiff.ps1`（比两帧，判断「这一下到底画没画出来」）、`scripts/topbar.ps1`（量出顶栏控件的真实 x 坐标——**别猜坐标**，按钮会随标签长短移动）。几条踩过的坑：坐标是**物理客户区像素**（150% 缩放下 CSS px × 1.5，所以「1400px 宽」其实只有 919 CSS px，会触发窄窗折叠）；窗口会恢复 `window.json` 因而可能一起来就是最大化，改尺寸前先 restore；**验证要另起一份便携目录**（拷 exe + 自己的 `data/`），别把窗口大小、进度写进真实的 `target/release/data`。截图输出放 `target/` 下（已 gitignore）。

- **阅读：** 无 `ICED_READER_OPEN` 进书架（看 **当前 exe 旁** 的 `data/library/`）。点封面能读、回书架进度还在。样书第一章中文、左右翻页、拉宽变双栏、关开后页大致还在。目录能跳、当前条高亮。F11 正文铺满，顶部热区能退出全屏，Esc 退出全屏。顶栏以下无空白条/灰底托窄白纸；窄窗顶栏仍约 52px。
- **字体：** 默认原书 CSS；四槽不齐时关掉原书字体正文不变。A± 变字号并写入 `settings.json`。五千年掌故：未安装的指定字体不要标成雅黑。新西游记：`cnepub` 标书内无字体文件，实际为 `（系统 serif）`。
- **封面：** 资治通鉴第一页整页背景图，横屏/最小窗口都整幅居中、不裁切（四周露纸色）；普通正文页不动。PDF 封面 = 第 1 页，与正文同样的居中留白。
- **划线：** 选字出现色板（默认黄=重点，绿=摘抄），下笔即定、不改色不调范围。点已有划线可删/写备注；有备注删除先确认，notes.md 留痕；纯划线删除无痕。重排、翻章、重启后高亮仍贴原句。iframe 保持 `about:srcdoc`。
- **词注：** `cargo test -p iced-reader-epub -- --ignored word_notes_expand_in_zztj`。注标是 CSS 画的、正文不加字；悬停能读完；`[N]` 能来回跳；无词注的样书正文不变。
- **PDF：** 打开 `.pdf` 进书架（页数、封面、**质量角标**：`Windows Everywhere….pdf` = 优、`經濟漩渦.pdf` = 良、`语文开窍….pdf` = 中；悬停看理由）；点封面进阅读应看到**一条连续纸带**（无 iframe、无灰底分栏），滚轮原生滚动、←/→ 翻页、PageDown/空格滚一屏、Home/End 首末页；**适应宽度 = 纸宽实时贴合窗口宽**（拖动窗口边缘，纸立刻跟着变宽；此模式恒单页）；**适应页面 = 整页贴合窗口高**，可切单页/双页/自动，双页为书式配对（第 1 页单独、之后 2-3 / 4-5），并排时进度区显示「· 跨页」；页码与进度跟着**视口面积最大的那张**走；目录跳页把目标页滚到视口顶部；进度区「· 跳页」输入第 N 页落到该页（越界夹到首/末页、非数字不跳且浮层不关）；无 outline 显示「本书没有目录」；退出重进回到同一页；改元数据改名后仍能打开、进度不丢；翻页要感觉即时（后台预取命中）；952 页的书滚动要顺、内存不随翻页无限涨（`loading="lazy"`）。含隐藏 OCR 文字层的扫描书不弹「缺字」提示；三本正文画质正常。
- **删书 / 元数据：** 三点菜单外点或 Esc 关闭；删除先确认；确认后 epub+md+进度+划线+notes+质量缓存都清掉。改元数据后面板预览与保存后标题一致，库内文件按显示名改名（**扩展名不变**），进度/划线不丢；同名 `.epub` 与 `.pdf` 互不影响。
