# Agent 说明

给在本仓库改代码的人（含编码助手）用。产品说明见 `README.md`。

## 这是什么

IcedReader 是 Windows 桌面电子书阅读器。名字不是 Iced GUI。当前实现：Tauri 2 壳 + React 界面 + Rust 核心。只做 Windows，不要把业务写进 Windows 专用 API（Tauri/WebView 边界留在 `src-tauri`）。

## 分层（不要打穿）

| 路径 | 可以做 | 不要做 |
| --- | --- | --- |
| `crates/core` | `Book` / `Locator` / 进度存储 | zip、OPF、Tauri、DOM |
| `crates/formats-epub` | 打开 EPUB、出 HTML 和资源 | UI、SQLite、直接给前端 rbook 类型 |
| `src-tauri` | 命令、文件对话框、`icedreader` 协议、便携目录、书库扫描（经 `BookOpener`） | 自己 unzip/解析 OPF、排版 |
| `ui` | 阅读壳、交互 | 读磁盘上的 epub 字节 |

新格式（TXT / PDF 等）只加适配器，实现 `Book` + `BookOpener`。前端命令形状尽量不动。

## 硬约束

1. **正文是 HTML。** 章节资源 URL 在 Rust 里改写成 `http://icedreader.localhost/book/{id}/...`，这是为了让书自己的图和 CSS 能加载，不是改版式。前端用 `srcDoc` 显示章节。**不要往章节里注入阅读皮肤 CSS 或其它装饰**（颜色、字号、`max-width` 居中等）。允许两处例外：① 用户关掉「使用原书字体」且 serif / sans / mono / 中文·CJK 四个上传文件都在并能识别时，注入 `@font-face`（CJK 用 `unicode-range`）并改写 `font-family`；缺任一槽位则整段不注入。每槽一个文件，磁盘名为 `serif` / `sans` / `mono` / `cjk` 加识别出的扩展名，是否启用看 `settings.json` 的登记，不要扫 `data/fonts/` 当导入。用户从字体面板上传；不要做成「往 fonts 文件夹丢文件即用」。`@font-face` 不写 `font-weight` / `font-style` 变体；覆盖开启后粗斜体走引擎合成。不要扩成 Regular/Italic/Bold 四文件族，也不要按文件名约定去配对。② 分页 flow：父页可写入带 `id="iced-reader-flow"` 的样式，只含高度、`column-*`、`overflow`、页边 `padding`、图 `max-width` / `max-height`、以及用户字号（`html { font-size: N% }`，N 为 80–160）。不要按书语言选日/韩字体，不要用系统字体。禁止黑色主题、禁止主题切换、禁止用 `prefers-color-scheme` 把壳或正文改成暗色。
③ **用户划线高亮（用户主动标注，非皮肤）：** 允许父页在章节 doc 里注入带 `id="iced-reader-highlight-style"` 的样式，只含 `::highlight(iced-reader-highlight)` 的背景色与 `color: inherit`，并用 CSS Custom Highlight API 上色；**不改 DOM 结构、不插 `mark`、不影响版式与内容**。选区捕获、点中判定、注入、删除全部由父页脚本完成（章节 iframe 仍不开 `allow-scripts`）。划线锚定按章内全局文本节点序号 + 节点内偏移（加原文摘录兜底），存 `data/annotations.json`；划线不属于「正文排版”，高亮随文字流动，翻页、字号变化不丢。
2. **进度只存 `Locator`：`href` + `fraction`（0～1）+ 可选 `cfi`。** 禁止存像素 `scrollTop`。CSS 分栏分页已有；`cfi` 仍留空，不要提前填假值。
3. **进度键：** 有 EPUB identifier 用 `id:...`；否则相对便携书库用 `lib:...`。禁止用会随目录搬家失效的绝对 `path:` 当主键（仅作没有书库目录时的回退）。实现见 `progress_key`。外部 EPUB 按文件名进书库，同名已存在则复用，不要每次打开复制成 `书名-2.epub`（没有 identifier 的书会因此丢进度）。`lib:书名-N.epub` 与 `lib:书名.epub` 视为同一本。
4. **章节 iframe 不要开 `allow-scripts`。** 现在是 `allow-same-origin`，父页做分栏翻页并读页序。不要为了省事给 EPUB 开脚本。
5. **IPC 用 camelCase**（Rust 结构体 `#[serde(rename_all = "camelCase")]`）。
6. **绿色软件：** 设置、进度、窗口位置、导入的书、用户字体、WebView 缓存一律在 `{exe 目录}/data/`，禁止写入 `%APPDATA%` / 注册表当主存储。打开外部 EPUB 时复制进 `data/library/`（已在书库内则不再复制）。字体文件在 `data/fonts/`，设置在 `data/settings.json`，窗口在 `data/window.json`，划线在 `data/annotations.json`（键同进度键）。整个程序目录搬走即带走全部状态。目录必须可写，不要往 Program Files 里装。不要用 tauri-plugin-window-state 之类会写用户目录的插件。

## 常用命令

```powershell
npm install
.\scripts\dev.ps1
cargo test -p iced-reader-core
cargo test -p iced-reader-epub
cargo test -p iced-reader
cargo check -p iced-reader
npx tsc --noEmit
npm run tauri -- build
```

自动打开样书：`$env:ICED_READER_OPEN = "$PWD\fixtures\sample.epub"`。

Windows 编译需要 MSVC。`scripts/dev.ps1` 会载入 vsvars；打 release 前同样要载入。用户说 build release 时：`npm run tauri -- build`，再用 `Copy-Item` 拷一份独立文件（禁止 Rename-Item / 硬链接）到 `target/release/IcedReader-{version}-windows-x64.exe`，`{version}` 取 `src-tauri/tauri.conf.json` 的 `version`。告诉用户这份路径；GitHub Release 上传它。不要把 `iced-reader.exe` 或 `deps/iced_reader.exe` 当发布文件。安装包会进 `target/release/bundle/`，不要当主分发方式。不要用 `--offline` 除非依赖已经在本地。只分发独立 exe 时可用 `npm run tauri -- build --no-bundle` 跳过安装包（免去 WiX/NSIS 下载与打包）；安装包 `targets: all` 在 Windows 含 MSI，打包工具 WiX 首次自动下载到 `%LOCALAPPDATA%\tauri\` 缓存，重跑不再下。

## Tauri 命令

- `open_book` / `close_book` / `pending_book` / `list_library` / `delete_book`（`fileName` + `progressKey`：删书库内 epub，并清该书进度与划线；前端必须先弹确认框再调）
- `get_chapter`（返回 `{ html, publisherFonts }`。`publisherFonts`：本章原书 CSS 的 font-family 原文、`@font-face` 名、以及 `src` 不在书内的 `unloadableFaces`）/ `resource_origin`
- `save_progress`
- `list_annotations` / `add_annotation` / `delete_annotation`（划线，键同进度键，存 `data/annotations.json`）
- `get_font_settings` / `set_use_original_fonts` / `set_font_scale` / `install_font` / `clear_font`
- `get_platform_fonts`（Chromium `CSS.getPlatformFontsForNode`，章节 iframe 里真正绘制用的字体）

协议：`src-tauri/src/protocol.rs`，scheme `icedreader`，实际请求是 `http://icedreader.localhost/...`。书架封面走 `/library-cover/{文件名}`，不要为此给章节 iframe 开脚本。启动无 `ICED_READER_OPEN` 时进入书架，不要再停在空白「打开一本 EPUB」。

## EPUB 章节

- **目录锚点当章。** 不少中文 EPUB 把多章放进同一 XHTML，NCX/`nav` 用 `#id`。TOC 带 fragment、或 TOC 条目明显多于 OPF spine 时，阅读列表用摊平后的 TOC（href 保留 `#`），`chapter_html` 按锚点切到下一 TOC 锚点。正规「一章一个文件」仍走 OPF spine。
- **路径改写要能过破烂 HTML。** rbook 按 XML 改写；未闭合 `<img>` 等会失败。失败后用宽松改写相对 `src`/`href`，不要为此给章节开脚本。
- **`lang`。** HTML 解析不认 `xml:lang`。章节 `srcDoc` 在没有 `lang` 时从 `xml:lang` 或书的 `dc:language` 补上（跳过 `und`），好让引擎按中文映射泛型 serif。不要为此注入阅读皮肤 CSS。
- **分页 flow。** 与 Foliate / Epub.js 相同：CSS 多栏 + `max-inline-size` 720px + `max-column-count` 2。栏数 `min(2, ceil(容器宽/720))`，竖屏强制一栏；多出的窗口宽度是页边，不拉宽正文。iframe 按页数拉宽，由外层容器 `scrollLeft` 翻页（不要在 `documentElement` 上滚）。左右键 / 点左右侧 / 滚轮翻页；章首再左翻上一章末页，章末再右翻下一章。进度仍是 `href` + 章内 `fraction`。

## 改 UI 时

- 阅读区铺满顶栏以下的客户区。正文块用 `.flow-host` 栅格居中（每栏最多约 720px），不要往章节 HTML 里注入 `max-width` / `margin: auto` 做居中。
- 字体面板要列出**本章原书 CSS 如何写 font-family**（含 serif/sans-serif/monospace 泛型、选择器、@font-face 名）。这是声明，不是系统最终选用的文件。在注入自定义字体之前从原 HTML/CSS 抽取。`@font-face` 的 `src` 若不是书内文件（如索尼 `res://`），标「书内无字体文件」，不要假装能加载。
- 字体面板还要列出**本章实际渲染字体**：
  - 不要用 canvas 在一堆系统字体里给泛型或未安装名「选最近的」（会把 serif 汉字标成雅黑、把未安装的 KaiTi 算进去）。
  - 命名字体：先认本机是否真有（西文字宽，或 `document.fonts` 且 `status === loaded`）。没装上的只列 CSS 里的名字（指定未安装）。
  - 栈里的 **CSS 泛型**（`serif` / `sans-serif` / `monospace` 等）实际生效时，显示 `（系统 serif）` 这类标签，来源为「泛型」。不要再猜宋体或雅黑。
  - 栈里既没有可用命名字体、也没有泛型时，才标缺字回退（`（系统 CJK 默认）` 或对上的已装 CJK 名）。
- 目录用 `book.toc`（没有则退回 spine 标题）。侧栏树、点条目跳到对应章首页。当前章高亮。不要在前端再 parse NCX。
- **书架：** `ui/src/Library.tsx` + `list_library`。封面用协议 `/library-cover/{文件名}`，URL 带 `coverRev`（文件大小+mtime），响应不要 `immutable` 长缓存，同名替换后应换图。不要在 JS 里 unzip，也不要把封面字节塞进 `list_library` 的 JSON。回书架前要 `await` 进度写入，再 `list_library`。删书已实现：封面右下三点按钮弹小菜单（目前仅「从书库删除」，走命令 `delete_book`），点菜单项后必须先弹确认框；菜单状态在 `Library.tsx` 内维护，不要另加删除入口。书库只扫 `data/library/` 一层 `*.epub`，不要做分类、子目录、内容哈希去重，除非产品明确要求。开发 `target/debug/data` 与 release `target/release/data` 是两套。
- **顶栏：** `.chrome` 固定 52px，`flex-wrap: nowrap`；按钮 `white-space: nowrap`；书名/作者/进度省略号。不要让长标题把工具栏撑高。窗口窄时（`≤1180px`）阅读视图把低频按钮（打开 EPUB / 目录 / 划线 / 字体 / 全屏，`.chrome-more`）收进右上「⋯」菜单（`.top-more` / `.top-menu`，菜单 `fixed` 定位避开 `.chrome` 的 `overflow: hidden`），书架、上一章/下一章、字号、书名保持常显；`.brand` 同阈值让位。不要用换行或横向滚动，不要整体藏掉章导航。
- **窗口几何：** 位置/大小/最大化存 `data/window.json`。全屏当阅读态，不要作为下次启动状态。不要在 `CloseRequested` 里 `prevent_close`。
- 全屏用 Tauri `setFullscreen`（F11 / 顶栏按钮），不要用浏览器 `requestFullscreen`；Windows 上关掉 WebView2 浏览器加速键，避免 F11 和引擎抢。Esc：先关目录，再退出全屏。全屏时顶栏默认收起，窗口顶部整条热区（含正文中间）可唤出，不要只靠左右页边的 mousemove。
- 界面文案默认中文。
- 提交信息用中文，说明做了什么、为什么。

## 不要做

- 不要把解析逻辑搬进 JS（包括让 foliate-js 直接 unzip）。分页若接 foliate-js，也只让它排版，书仍由 Rust 打开。
- 不要为了第一口去接 PDF/MOBI；接口预留即可。
- 不要做黑色主题、暗色模式、主题切换。本软件就是浅色纸。
- 不要提交 `target/`、`node_modules/`、`ui/dist/`、`src-tauri/gen/`、`data/`、`fixtures/verify-*.png`、仓库根目录的本地 EPUB（`fixtures/sample.epub` 除外）。
- 不要在文档或代码里写用户的密钥、本机绝对路径（样例用仓库相对路径）。

## 验证

改阅读功能时：无 `ICED_READER_OPEN` 时启动应进书架（看 **当前 exe 旁边** 的 `data/library/`，开发是 `target/debug/data`）。点封面打开，顶栏「书架」能回去且进度还在。打开 `fixtures/sample.epub`，确认第一章中文、左右翻页、拉宽窗口变双栏、关开后页大致还在。点「目录」能跳章，当前条目高亮。F11 全屏后正文铺满，鼠标移到顶部能再点「退出全屏」，Esc 退出全屏。改布局时确认顶栏以下没有空白条、没有灰底托一条窄白纸；窄窗口顶栏高度仍约 52px、文字不竖排。改字体时：默认仍是原书 CSS；只传部分字体并关掉「使用原书字体」时正文不变；四槽都齐才覆盖，CJK 字走中文/CJK 槽。字号 A−/A+ 应变大变小并写入 settings.json，不要出现暗色主题。可用仓库旁未提交的样书核对（不要 git add）：`五千年掌故.epub` 指定 PingFang SC / FZFangSong-Z02，Windows 上通常未安装；`新西游记++共两册.epub` 指定 `cnepub, serif` 但 `@font-face` 是设备 `res://`，实际渲染应为 `（系统 serif）`，并标 cnepub 书内无字体文件。没有桌面窗口时至少跑 `cargo test -p iced-reader-core`、`cargo test -p iced-reader-epub`、`cargo test -p iced-reader`。

改划线时：选中一段文字松手应浮出「划线」，点后即时变黄；再点这段（无选区单击）浮出「删除划线」；双击已有划线中的词也应是删除而非报重叠；删除后即刻消失。A±/拉宽窗口后划线仍贴在原句上。翻走再回来、重启程序后划线仍在，且 `data/annotations.json` 已更新。选中文字点划线后不应翻页（点正文中段空白处翻页仍正常）。顶栏「划线」应列出全书画线（按阅读顺序），点一条跳到所在章的那一页（跨章会切章，同章直接定位），列表里删除后即时消失。

改书架删书时：封面右下三点弹出小菜单，点菜单外 / Esc 应关闭；「从书库删除」先弹确认框，取消不动书；确认后卡片消失，`data/library/` 里文件没了，`progress.json` / `annotations.json` 里该书（同 stem 的 `lib:` 键）记录也清掉；同名重新导入进度从零开始。坏书（无法打开）也能删。

改正文链接时：点书内注文/目录链接应在阅读器内跳转（同文件锚点定位到那一页，跨文件切章），iframe 的 `contentDocument` 必须仍是 `about:srcdoc`——若 iframe 被链接导航走会跨源、布局变坏且无法翻页。用注文多的书（如东周列国志）点 `[n]` 链接核对。
