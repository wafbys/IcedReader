# Agent 说明

给在本仓库改代码的人（含编码助手）用。产品说明见 `README.md`。这里只留禁则和入口；实现契约写在对应模块注释里，不要把同一条事实再抄一遍。

## 这是什么

IcedReader 是 Windows 桌面电子书阅读器（不是 Iced GUI）。栈：Tauri 2 + React + Rust。只做 Windows；业务不要写进 Windows 专用 API（Tauri/WebView 边界留在 `src-tauri`）。

## 分层（不要打穿）

| 路径 | 可以做 | 不要做 |
| --- | --- | --- |
| `crates/core` | `Book` / `Locator` / 进度存储 | zip、OPF、Tauri、DOM |
| `crates/formats-epub` | 打开 EPUB、出 HTML 和资源 | UI、SQLite、直接给前端 rbook 类型 |
| `src-tauri` | 命令、文件对话框、`icedreader` 协议、便携目录、书库扫描（经 `BookOpener`） | 自己 unzip/解析 OPF、排版 |
| `ui` | 阅读壳、交互 | 读磁盘上的 epub 字节 |

格式层按 `Book` + `BookOpener`（目前只有 EPUB）。解析/排版不进 JS；前端命令形状尽量不动。

## 硬约束

1. **正文是 HTML。** 资源 URL 在 Rust 里改写成 `http://icedreader.localhost/book/{id}/...`，前端用 `srcDoc` 显示。**禁止往章节注入阅读皮肤**（颜色、装饰性字号、字体族、行距、`max-width` 居中、主题）。禁止黑色主题、禁止主题切换、禁止用 `prefers-color-scheme` 把壳或正文改暗。父页只允许这些注入（合法内容以模块注释为准）：
   - 四槽字体齐且关掉「使用原书字体」：`@font-face` + 改写 `font-family`（`crates/core/src/fonts.rs`、`settings.rs`）
   - `#iced-reader-flow` 栏式分页（`ui/src/flowLayout.ts`）
   - `#iced-reader-highlight-style` 用户划线（`ui/src/highlights.ts`）
   - `#iced-reader-note-style` 词注呈现（`ui/src/wordNotes.ts`；展开在 `crates/formats-epub/src/footnotes.rs`）
   - `#iced-reader-cover-fit` 近空章整页背景封面（`ui/src/coverFit.ts`）
2. **进度只存 `Locator`：** `href` + `fraction`（0～1）+ 可选 `cfi`。禁止 `scrollTop`。`cfi` 留空，不要填假值。
3. **进度键：** 有 identifier 用 `id:...`；否则 `lib:...`（相对便携书库）。`path:` 仅无书库目录时回退。外部 EPUB 按文件名进 `data/library/`，同名复用、不另存 `-2`。`lib:书名-N.epub` 与 `lib:书名.epub` 视为同一本。见 `progress_key` / `same_book`。
4. **章节 iframe 只要 `allow-same-origin`，不要 `allow-scripts`。** EPUB 内容不开脚本。书内链接拦在父页，iframe 必须保持 `about:srcdoc`。
5. **IPC camelCase**（`#[serde(rename_all = "camelCase")]`）。
6. **绿色软件：** 状态一律 `{exe}/data/`，禁止 `%APPDATA%` / 注册表当主存储，不要用会写用户目录的 window-state 插件。目录必须可写，不要装进 Program Files。文件布局见 `src-tauri/src/portable.rs`。

## 常用命令

```powershell
npm install
.\scripts\dev.ps1
cargo test -p iced-reader-core
cargo test -p iced-reader-epub
cargo test -p iced-reader
npx tsc --noEmit
```

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

## 改 UI 时

- 阅读区铺满顶栏以下。正文用 `.flow-host` 栅格居中，不要往章节 HTML 里注入 `max-width` / `margin: auto`。
- 字体面板：原书 CSS 声明 vs 本章实际绘制，规则见 `ui/src/usedFonts.ts`。在注入自定义字体之前从原 HTML/CSS 抽取；`src` 不在书内的 `@font-face` 标「书内无字体文件」。
- 目录用 `book.toc`（否则 spine 标题）。不要在前端 parse NCX。
- 书架：`ui/src/Library.tsx`。封面走 `/library-cover/{文件名}` + `coverRev`，不要 `immutable` 长缓存，不要在 JS 里 unzip / 把封面字节塞进 `list_library`。回书架先 `await` 进度再 `list_library`。封面三点菜单（悬停才显示）：「编辑元数据…」在上、「从书库删除」在下（先确认）。统计行、质量角标、同书提示由 `list_library` 字段纯前端算；`list_library` 不在热路径重算质量。书库只扫 `data/library/` 一层 `*.epub`。开发 `target/debug/data` 与 release `target/release/data` 是两套。
- 顶栏 `.chrome` 固定 52px、不换行；窄窗口（≤1180px）把低频按钮收进右上「⋯」。不放品牌字。全屏用 Tauri `setFullscreen`（F11），关掉 WebView2 浏览器加速键。Esc：先关浮层/目录，再退出全屏。全屏顶栏默认收起，窗口顶部整条热区可唤出。位置/大小/最大化存 `data/window.json`；全屏不当下次启动状态。不要在 `CloseRequested` 里 `prevent_close`。
- 界面文案默认中文。提交信息用中文，说明做了什么、为什么。

## 不要做

- 不要把解析逻辑搬进 JS（包括让 foliate-js 直接 unzip）。分页若接 foliate-js，也只让它排版，书仍由 Rust 打开。
- 不要做黑色主题、暗色模式、主题切换。本软件就是浅色纸。
- 不要提交 `target/`、`node_modules/`、`ui/dist/`、`src-tauri/gen/`、`data/`、`fixtures/verify-*.png`、仓库根目录的本地 EPUB（`fixtures/sample.epub` 除外）。
- 不要在文档或代码里写用户的密钥、本机绝对路径（样例用仓库相对路径）。

## 验证

改哪测哪。没有桌面窗口时至少跑 `cargo test -p iced-reader-core`、`iced-reader-epub`、`iced-reader`。样书：`fixtures/sample.epub`；仓库旁未提交的 `资治通鉴.epub` / `五千年掌故.epub` / `新西游记++共两册.epub` 不要 git add。

- **阅读：** 无 `ICED_READER_OPEN` 进书架（看 **当前 exe 旁** 的 `data/library/`）。点封面能读、回书架进度还在。样书第一章中文、左右翻页、拉宽变双栏、关开后页大致还在。目录能跳、当前条高亮。F11 正文铺满，顶部热区能退出全屏，Esc 退出全屏。顶栏以下无空白条/灰底托窄白纸；窄窗顶栏仍约 52px。
- **字体：** 默认原书 CSS；四槽不齐时关掉原书字体正文不变。A± 变字号并写入 `settings.json`。五千年掌故：未安装的指定字体不要标成雅黑。新西游记：`cnepub` 标书内无字体文件，实际为 `（系统 serif）`。
- **封面：** 资治通鉴第一页整页背景图，横屏/最小窗口都整幅居中、不裁切（四周露纸色）；普通正文页不动。
- **划线：** 选字出现色板（默认黄=重点，绿=摘抄），下笔即定、不改色不调范围。点已有划线可删/写备注；有备注删除先确认，notes.md 留痕；纯划线删除无痕。重排、翻章、重启后高亮仍贴原句。iframe 保持 `about:srcdoc`。
- **词注：** `cargo test -p iced-reader-epub -- --ignored word_notes_expand_in_zztj`。注标是 CSS 画的、正文不加字；悬停能读完；`[N]` 能来回跳；无词注的样书正文不变。
- **删书 / 元数据：** 三点菜单外点或 Esc 关闭；删除先确认；确认后 epub+md+进度+划线+notes+质量缓存都清掉。改元数据后面板预览与保存后标题一致，库内文件按显示名改名，进度/划线不丢。
