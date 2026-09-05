# IcedReader

Windows 桌面电子书阅读器。产品名沿用 IcedReader，技术栈是 **Tauri 2 + React + Rust**，正文用系统 WebView 渲染 EPUB 的 HTML。

当前只做 Windows（x64）。目前只出品 EPUB。

**没有黑色主题，也不会做。**

## 现在能做什么

- **书架：** 启动进入本地书架（无 `ICED_READER_OPEN` 时）。列出 `data/library/` 里的 `.epub`（书名、作者、封面、章节进度、质量；最近读过的在前，未读的按质量排序）。点封面继续读。封面右下角三点菜单：可「编辑元数据…」（见下条）或「从书库删除」——删除前会弹确认框，确认后连同这本书的进度与划线一起清除。阅读顶栏「书架」返回。首次导入会分析排版质量（界面显示进行中提示）：封面左上角标 优/良/中（悬停看理由）；若与书库中已有书为同一本（重打包或同版），只提示「同书 ×N」，不自动处理。网格上方有轻量统计行（共 N 本 · 未读 · 已打开 · 无法打开）。尚无分类、子目录。
- **书名规范（编辑元数据）：** 三点菜单「编辑元数据…」打开面板，可编辑主书名 / 副标题 / 卷册 / 作者 / 译者 / 出版年份 / 出版社 / ISBN，拼接预览实时显示；「显示名」留空 = 自动按 `书名 _ 副标题 - 卷册 - 作者 - 译者 - 年份 - 出版社 - ISBN` 拼接（下划线只用于书名与副标题之间，其后用 ` - `；空字段自动跳过；书名必填），填写即锁定、字段改动与自动填充都不覆盖它。保存后书架、阅读顶栏与 `data/library/` 里的文件名一致（epub 与伴生 md 按最终显示名改名，Windows 禁用作文件名清洗、同名自动加 `-2`；进度与划线不丢）。面板还能「重新读取原书元数据」——一键清空手填、用原书书名/作者/出版社/ISBN 回填，是否保存由你决定。元数据存在同目录 `<书名>.md`（程序维护，可备份），删除书时一并清除；作者/译者多名用半角逗号连接，书名里不出现程序产生的中文标点。
- **打开书：** 本地 EPUB 2 / 3。「打开 EPUB」会复制进 `data/library/`（同名已存在则复用，不另存 `-2`）。阅读顶栏显示书名、作者、当前章与页。
- **分页：** Foliate / Epub.js 式 CSS 分栏。每栏正文最多约 720px；窗口够宽且横屏时双栏，多出的宽度当左右页边。正文上下约 20–40px 留白。左右键 / 滚轮翻页；章边界再进上一章或下一章。目录用锚点标在同一文件里的章会拆开翻。正文里的书内链接（古籍的 `[1]` 注文等）在阅读器内跳转——同文件锚点翻到注文页，跨文件切章，不会把正文导航乱。
- **书内词注：** 微信读书导出类 EPUB（如《资治通鉴全本注译》）把词注塞在空 `<span data-wr-footernote="…">` 属性里，普通阅读器看不到任何注。IcedReader 在 Rust 排版层把它们原位展开成被注词后的小上标序号：**悬停弹出黑底白字浮层读完注文全文**，**点击跳到本段后的注文块**（注文开头可返回正文）；正文没有因此加字、章节 iframe 仍不开脚本。无此类注的书完全不受影响。
- **目录：** 侧栏树，点击跳到该章；当前章高亮。
- **全屏：** 顶栏「全屏」或 F11；Esc 先关目录再退出全屏。全屏时顶栏收起，鼠标移到窗口顶部整条热区再出现。
- **进度：** 只存章节 `href`（可含 `#锚点`）+ 章内比例 0～1，不存像素。有 EPUB identifier 用 `id:...`，否则 `lib:文件名`。
- **窗口：** 默认 1120×780，最小 800×520。窗口标题为 `IcedReader {版本号} ({build 时 git 短 hash})`——hash 由 build 时固化，方便对发布件溯源（无 git 环境构建时只显示版本号）。关闭时记住位置、大小和是否最大化（`data/window.json`），下次打开还原；全屏不记住。顶栏固定 52px；窄窗口或长书名用省略号，不换行撑高。阅读时窗口过窄（≤1180px）顶栏把低频按钮（打开 EPUB / 目录 / 划线 / 字体 / 全屏）收进右上「⋯」菜单，书架、上一章/下一章、字号、书名保持常显。
- **绿色软件：** 书、进度、字体、设置、WebView 数据都在 exe 同级 `data/`。开发跑 `target/debug/`，release 跑 `target/release/`，**两套 data 互不相通**。拷走整个程序目录即带走状态。不要装进 Program Files。
- **字号：** 顶栏 A− / A+，80%–160%，步进 10%，默认 100%。记在 `settings.json`。只改分页注入的 `html` 字号百分比，不往章节里灌阅读皮肤。原书写死 `px` 的不一定跟着变。
- **划线：** 选中正文文字，松手浮出「色板 + 划线」：**黄 = 重点**（默认，思考/存疑等一切有想法的标注都归它）、**绿 = 摘抄**（纯摘录）。下笔即定，不做改色/调范围（要换 = 删除重划）；按钮底色即所选色，所见即所得。划线是**书外之物**：不改章节 DOM、不动版式，用 CSS Custom Highlight 在原文上着色，随字号/重排/翻页/重启保持贴在原句。锚定按章内文本节点序号（加原文摘录兜底），存 `data/annotations.json`（键与阅读进度同，记录含颜色与全书位置）。与已有划线重叠的选区不允许新建（先删旧的）。
- **备注（划线笔记）：** 点已有划线浮出「✎ 备注」，写一段备注保存后，划线与备注一起落入 `data/library/<书名>.notes.md`（按章分组：程序保护区 + 你的自由笔记区，可在外部 md 软件继续编辑；程序重写只动保护区、你的文字原样保留）。**只有写了备注的划线才进 notes.md**；鼠标悬停正文划线会浮现备注内容（尽量完整，多可滚动）。删除划线前若有备注会先提示：正文高亮消失，但划线内容与备注**保留在 notes.md 并记删除时间**（像会计不涂改）；纯划线删除即无痕。顶栏「划线」列出全书画线（色块 + 重点/摘抄标签 + 划线时间 + 备注预览，按阅读顺序），点一条跳到所在章的那一页（跨章会切章，同章直接定位），列表里也能删除。
- **按全书位置跳转：** 顶栏进度右侧显示「全书 N%」（按各章正文实际字符数加权），点它输入 0–100 回车即跳到全书对应位置并保存进度——读长书（资治通鉴式 294 章）的第二种导航。
- **字体：** 默认「使用原书字体」。关掉且衬线 / 无衬线 / 等宽 / 中文·CJK 四个文件都经**字体面板**上传后，才覆盖（CJK 码位走中文槽）。缺任一槽则仍按原书 CSS。每槽一个文件（Regular 或 Book 即可）。覆盖后斜体粗体由引擎合成。只把文件丢进 `data/fonts/` 不会生效。字体面板分两栏：原书 CSS 怎么写，以及本章实际绘制（泛型显示「（系统 serif）」，不猜宋体/雅黑）。

样书：`fixtures/sample.epub`。

## 接着要做

章内搜索。分页已实现；`Locator.cfi` 仍预留，未填写。划线已支持 黄（重点）/绿（摘抄）两色 + 备注 + notes.md 档案 + 删除留痕；后 v1 候选：划线范围调整（现范围固定，调整 = 删除重划；若真需要，改范围时笔记随条目保留）。不做主题 / 暗色模式。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面壳 | Tauri 2（WebView2） |
| 界面 | React 19 + TypeScript + Vite |
| 领域与解析 | Rust：`iced-reader-core`、`iced-reader-epub`（rbook） |

业务逻辑进 Rust 核心，不进 Windows API。前端不直接拆 `.epub`。

## 环境

- Windows 11（已带 WebView2）
- [Rust](https://rustup.rs/)（MSVC 工具链）
- Visual Studio 2022 Build Tools，勾选「使用 C++ 的桌面开发」
- Node.js LTS

若本机开着 Smart App Control（强制），会拦住 Cargo 的未签名 build script，也会拦住本地编出来的 `iced-reader.exe`。开发前请先关闭。

## 开发

```powershell
npm install
.\scripts\dev.ps1
```

或：

```powershell
npm run tauri -- dev
```

调试时自动打开一本书：

```powershell
$env:ICED_READER_OPEN = "$PWD\fixtures\sample.epub"
.\scripts\dev.ps1
```

便携数据（相对 exe 所在目录）：

```
iced-reader.exe
data/
  library/          导入的书；划线+备注档案 <书名>.notes.md 也在这（删除留痕）
  fonts/            字体面板写入的 serif/sans/mono/cjk 文件
  settings.json     阅读设置（含「使用原书字体」和各槽登记）
  window.json       窗口位置、大小、是否最大化
  progress.json     阅读进度
  annotations.json  划线（键同进度键，含颜色/全书位置）
  book-signals.json 同书指纹与质量信号缓存（含每章字符权重）
  webview/          WebView2 用户数据
```

开发时 exe 在 `target/debug/`，字体和书库在 `target/debug/data/`。release 的 `IcedReader-…-windows-x64.exe` 用的是 `target/release/data/`（没有则自行生成空目录）。两套不要混用，除非你把整个 `data` 拷过去。

## 发布

只构建绿色版（独立 exe）。先载入 MSVC（与 `scripts/dev.ps1` 相同，或在「x64 Native Tools」终端里）：

```powershell
npm run tauri -- build --no-bundle
```

产物为 `target/release/iced-reader.exe`。编完后必须再拷一份**独立文件**（不要改名硬链接），供分发和 Everything 检索：

```powershell
Copy-Item target\release\iced-reader.exe target\release\IcedReader-0.9.0-windows-x64.exe
```

文件名：`IcedReader-{version}-windows-x64.exe`，版本号与 `src-tauri/tauri.conf.json` 的 `version` 一致。

| 产物 | 路径 |
| --- | --- |
| 分发用绿色 exe | `target/release/IcedReader-{version}-windows-x64.exe` |
| Cargo 原始 exe | `target/release/iced-reader.exe`（可留给工具链，不要当发布文件） |

GitHub Release 上传那份 `IcedReader-…-windows-x64.exe`。拷到任意可写目录再运行，同级生成 `data/`。不要装进 Program Files，否则可能写不进 `data/`。系统仍需 WebView2（Win11 自带）。

## 测试

```powershell
cargo test -p iced-reader-core
cargo test -p iced-reader-epub
cargo test -p iced-reader
npx tsc --noEmit
```

## 目录

```
crates/core           格式无关的书模型、进度
crates/formats-epub   EPUB 适配器（rbook 不外泄到前端）
src-tauri             Tauri 命令、自定义协议 icedreader://、书库扫描
ui                    书架（`Library.tsx`）/ 阅读壳（划线逻辑在 `highlights.ts`）
fixtures              样书
scripts/dev.ps1       Windows 开发启动
```

给编码助手的仓库约定见仓库根目录的 [`AGENTS.md`](AGENTS.md)，新开对话会自动读入并遵循。

## 许可

MIT
