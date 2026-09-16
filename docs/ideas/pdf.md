# 想法/进展：PDF 支持（第二个格式）

状态（2026-09-15 起稿）：**第一期已实现（Rust 侧完成并通过测试；前端 UI 见下）；spike 已通过**。用户 2026-09-15 拍板：引擎走**纯 Rust 栅格化（hayro）**、**第一期只读**、书架 EPUB/PDF 混排、**先跑 spike 再开第一期**、**无 outline 就不给目录**、样本 PDF 由用户放仓库旁不入库；pdf.js 与 pdfium 两条更重的路线留作后续（见「为什么不是 B / C」）。

## 第一期实现现状（2026-09-15）

已完成（`cargo test`：core 49 / epub 24+2 / pdf 10+1 / lib 53 全过；`cargo build -p iced-reader` 通过）：

- `crates/formats-pdf/src/book.rs`：`PdfOpener` / `PdfBook` 实现 `Book`/`BookOpener`——一页一个 spine 单元（`page/0001`）、outline→`TocNode`、页 `title` 取最近的目录条目、`resource` 解析 `page/NNNN.webp?w=` 栅格化一页并走内存 LRU（`CACHE_PAGES = 16`）、`page_sizes()` 给前端整条纸带的占位尺寸（`chapter_html` 只剩别的调用方用，壳不再为 PDF 调 `get_chapter`）。
- `crates/formats-pdf/src/lib.rs`：`PdfDoc`（页数 / Info / outline / 字体体检 / 单页算子普查 / 栅格化 + ink）、`cover_png`（第 1 页 400px）、`visible_text_risk`（可见文字 + 未嵌入字体才算风险）。
- `crates/core`：`PDF_FORMAT` / `BOOK_EXTENSIONS` / `book_stem` / `book_extension`；`progress.rs` 的 `lib_book_stem` 改为「保留扩展名 + 剥 `-N` 副本后缀」，因此同名 `.epub` 与 `.pdf` 是两本书（+2 个单测）。
- `src-tauri/src/openers.rs`：格式分派唯一入口（`openers()` / `opener_for` / `open_any` / `is_supported`，带单测）。
- `src-tauri/src/library.rs`：书库扫 `*.epub` + `*.pdf`；`profile_book` 按 opener 分派（PDF 的 `has_cover = true`、不评质量）；`cover_bytes` 分派（PDF 渲染第 1 页）；改名保留扩展名；`unique_stem`/`stem_taken` 认所有书籍扩展名。
- `src-tauri/src/lib.rs`：`open_book` 分派 + PDF 跳过 signal 分析、`chapterChars = 每页 1`、返回 `warnings`；`get_chapter` 对 PDF 不做字体注入；`reread_book_meta` 分派；`set_book_meta` 的 `lib:` 键迁移按实际扩展名。
- `src-tauri/src/protocol.rs`：把 `uri().query()` 拼回资源 href（否则 `?w=` 通道是死的）。
- `src-tauri/src/portable.rs`：导入兜底名不再写死 `.epub`。
- 文档：README（产品说明、技术栈、目录、便携数据、测试命令）、AGENTS（分层表、合法注入清单、进度键扩展名规则、新增「PDF」小节、验证清单、常用命令）。

剩余（手工核对）：~~桌面窗口验证~~ **已完成（2026-09-16，见「第一期桌面验证」）**。前端已按 SumatraPDF 式连续纸带重写（见下一条与「反馈改进」）。

### 第一期收尾时的两处性能修正（2026-09-15）

- **`PdfDoc` 常驻已解析的 `Pdf`**：原先每渲染一页都 `Pdf::new(bytes.clone())`——31 MB 的书等于每翻一页克隆 31 MB + 重新解析 5~34 ms，而真正栅格化只要 14~20 ms。改成 `open` 时解析一次、复用（`PdfDoc: Send + Sync`，已有编译期断言），并且不再保留原始字节副本（大 PDF 内存占用减半）。实测：英文书正文页 14~20 ms、扫描页 84~89 ms，`parse_ms` 恒为 0。
- **书锁不再跨栅格化**：`AppState.books` 改为 `Mutex<HashMap<String, Arc<dyn Book>>>`，`get_chapter`/协议 handler/`save_note` 先克隆 `Arc` 再放锁。此前协议 handler 持全局书锁同步渲染（重首页 0.7~1.7 s），会堵住其它书的操作。

## 第一期反馈改进（2026-09-15，用户验收后）

用户四条反馈 → 处理：

1. **「PDF 渲染性能不高」** — 先量清楚，两个直觉都被数据否掉，第三个（WebP）成立：
   - **JPEG 是负优化**：1440px 同页，`image` 的 baseline JPEG 编码 **65–95 ms（比 PNG 慢 6~9 倍、比 WebP 慢 3~4 倍）**，字节还比 WebP 多 → 永不使用。
   - **无损 WebP 胜出，已改为默认**：文字页 PNG 15.6 ms / 498 KB → **WebP 20.3 ms / 224 KB**；扫描页 PNG 19.8 ms / 1042 KB → **WebP 27.5 ms / 324 KB**。编码多的 5–8 ms 发生在**预取线程**里（不占翻页路径），换来 55–69% 的字节、同比例的缓存内存与更快的 webview 解码。封面 400px 也小 16–45%（例如 453 KB → 293 KB）。
   - **`hayro-syntax` 的 `unsafe` 特性（SIMD/memchr/flate2）实测无收益**：98.1 / 45.3 / 23.3 ms（关）vs 110.1 / 46.4 / 28.2 ms（开）→ 关掉。
   - **真正的成本**：扫描页栅格化里有一块约 40–60 ms 的**内嵌图像解码固定成本**（800px 64 ms → 1440px 87 ms → 2160px 112 ms，不随像素线性）；文字页便宜得多（1000px 15.5 ms / 1440px 21 ms）。
   - **已做的四件事**：① **后台预取**（一次页请求排队它左右各 2 页，独立 worker 线程 + 在飞集合去重）；② **栅格宽度交给调用方**：`?w=` 由前端按「纸实际显示多大 × DPR」算出（降 `w` 是扫描页唯一有效手段）；③ **宽度分桶**（Rust 端 `WIDTH_STEP = 128px`，前端量化 120px）：窗口缩放会算出略不同的宽度，不分桶就会每次缓存落空重新栅格化；④ 缓存 16 页内存 LRU（WebP 下约 0.2–0.35 MB/页，合计约 5 MB）。
   - **预取实测**（release，宽度 1280 桶，三本样本，WebP；「窗口」列是当时那版窗口机制的测量，机制后来删除了，这列现在只作"连续 N 页都能缓存命中"的能力参考）：

     | 样本 | 冷页首次 | 预取后的相邻页 | 5 页窗口合计 |
     | --- | --- | --- | --- |
     | Windows Everywhere（真文字） | 22.0 ms | **0.1 ms** | 0.2 ms |
     | 經濟漩渦（扫描 + OCR 层） | 143.1 ms | **0.2 ms** | 2.0 ms |
     | 语文开窍（纯扫描） | 91.8 ms | **0.2 ms** | 0.7 ms |

     即：**只有每个窗口的第一页付栅格化成本，翻页与窗口内其余页都是缓存命中**。
   - **刻意不做「开书预热」**：宽度由前端的容器宽 × DPR 决定，开书时猜一个宽度只会渲染出随后被重新渲染的废页；第一页请求会按**它自己的宽度**预热左右邻居。
   - 仍未做：批量渲染共享 `RenderCache`（字体/图像缓存跨页复用，只对文字书有收益）。
2. **「PDF 不考虑换字体」— 已拍板，不做设置项**。诊断入口保留（`PdfDoc::render_page_png_with`、`--substitute-font`），实现在「附」一节。
3. **「适应宽度应该连续滚动」+「适应页面给出双页选项」——定稿为 SumatraPDF 式连续纸带**（用户 2026-09-15 进一步拍板：「**PDF 不是 EPUB**」「适应宽度就是动态适应窗口的宽度」）。
   - 中间做过一版「页窗口 `page/0007@5` + 滚到边界换段」，那是拿 EPUB 的分栏分页器去凑 PDF，**已整体删除**（Rust 的 `@N` 窗口机制、前端 `pdfLayout.ts` 与 `pdfPaging` 的换段逻辑都不在了；配对/比例/栅格宽度的纯函数保留）。
   - 现在的实现：`ui/src/PdfView.tsx` 在父页里画一条纵向连续纸带（`.pdf-scroll/.pdf-strip/.pdf-row/.pdf-sheet`，**无 iframe、无分栏、不调 `get_chapter`**），一「行」放 1~2 张纸，配对书式（封面单独、之后 2-3 / 4-5）；**缩放随窗口实时重算**（ResizeObserver）：适应宽度 = 纸宽贴合阅读区宽且**恒单页**，适应页面 = 纸高贴合窗口高，双页「自动」按真实页面比例判断；滚轮原生滚动、←/→ 翻页、PageDown 滚一屏、Home/End 首末页；**当前页 = 视口面积最大者** → 顶栏「第 N / M 页」与进度（`href = page/NNNN`、`fraction = 0`）。
   - Rust 侧为它加了 `Book::page_sizes()`（PDF 全部页尺寸、EPUB 为空）→ `OpenedBook.pageSizes`，让**整条纸带的占位**（滚动条长度与跳页精度）从第一帧就正确，图片再懒加载。
5. **「Windows Everywhere 没有质量评价」** — 新增 PDF 专用信号与评分，见下。

### PDF 质量评价（新增）

`crates/formats-pdf/src/quality.rs` 测：文本层类型（真文字 / 扫描+OCB 隐藏层 / 纯扫描 / 混合，按抽样页的 `Tr` 可见性判定）、字体嵌入数、**可见文字用了未嵌入字体**的名单、目录条数、书名/作者。`src-tauri/book_signals.rs` 的 `analyze_pdf` 存进同一份 `book-signals.json`（`BookSignals.pdf`），`grade_pdf` 按用户口径打分：

| 样本 | 判定 | 理由 |
| --- | --- | --- |
| `Windows Everywhere….pdf` | **优** | 952 页 · 正文可提取文字 · 6 个字体全部嵌入 · 含书签目录 148 条（无扣分项） |
| `經濟漩渦.pdf` | **良** | 扫描 + OCR 文字层（可搜但版面是扫描图）· 17 个字体未嵌入 · 无书签目录 |
| `语文开窍….pdf` | **中** | 406 页 · 有目录 71 条，但纯扫描、无可提取文字 |

顺带修掉一个隐患：`BookSignals.fingerprint` 为空时原来的「同书」分组会把所有书归成一本（PDF 若不给指纹就会踩中），现在空指纹不参与分组；PDF 自己用「页数 + 书名 + 目录标签 + producer」的粗指纹。

## 第一期桌面验证（2026-09-16）

在 release 绿色版里按 AGENTS.md 的验证清单逐项过了一遍。**另起一份便携目录**（`target/verify/app/`，exe + 自己的一份 `data/`）跑，不碰用户真实书库。

先说一个前提：验证前 `target/release/IcedReader.exe` 比源码旧了约一小时（22:34 的产物 vs 23:38 的提交），**先重新构建**才验。

通过项：

- **书架**：EPUB/PDF 混排，封面、页数、进度条、统计行正常；手工塞一个 17 字节的坏 `.pdf` → 显示「无法打开」，三点菜单能删（删除后书架回到 5 本）。
- **质量角标**：打开过的书才有角标（信号在 `open_book` 里后台算，`list_library` 只读缓存——**从没打开过的书不会猜一个分数**）。实测 `Windows Everywhere`=优、`經濟漩渦`=良、`语文开窍`=中、`控制论与科学方法论`=中，悬停出的理由与 `grade_pdf` 口径一致。
- **阅读面**：一条连续纸带（无 iframe、无分栏）；滚轮原生滚动、←/→ 翻页、PageDown/Space 滚一屏、Home/End 首末页全对；**适应宽度**=纸宽贴合阅读区宽且恒单页（张数组自动隐藏）；**适应页面+自动**=书式配对（封面单独、2-3、4-5），末页落单时只有一张；目录跳页把目标页所在行滚到视口顶部并高亮当前条；无 outline 的书侧栏显示「本书没有目录。」；`經濟漩渦`（隐藏 OCR 文字层）**不弹缺字提示**；三本正文画质正常。
- **进度**：`page/NNNN` + `fraction 0` 落盘，退出重进回到同一页；改元数据改名后库内文件按显示名改名（**扩展名不变**）、`progress.json` 键迁移到新 `lib:` 名、页码保持（476 → 打开仍在 476）。
- **翻页手感**：`→` 之后 <1 s 抓的帧与 2.5 s 后完全一致，即目标跨页已画好（预取命中）。
- **EPUB 回归**：打开/翻页/目录/划线面板/字体面板/顶栏进度全无变化。

### 顶栏两处修正（本次验证揪出来的）

都在进度区，前端 3 个文件：

1. **「· 跨页」以前看的是「模式」而不是眼前这一行**：`pdfState.spread` 是 `resolveSpread()` 的结果，于是封面（书式配对下第 1 页单独一张）和末页落单时也写着「跨页」。改为上报**当前行实际有几张纸**（`PdfViewState.paired` ← `layout.rows[index].length > 1`），`App.tsx` 用它决定是否加「· 跨页」。
2. **长标题把页码和「全书 N%」整个挤掉**：`.pos` 原本整条 `nowrap + overflow:hidden`，PDF 的 outline 标题动辄二三十字 → 页码被截成「第 …」，「全书 N%」跳转按钮**被裁到点不到**（默认的「适应页面 + 自动/双页」下张数组占宽，即使标题不长也会裁）。改成 `.pos` 是 flex：只有标题那一段（`.pos-title`）可缩并省略，页码（`.pos-now`）与跳转按钮 `flex: none` 常显，`.pos` 保留 `overflow:hidden` 只在窗口极窄时兜底。实测：封面页显示「第 1 / 952 页 · 全书 0%」（不再有「跨页」），跳到长标题章节后「第 4 / 952 页 · 跨页 · 全书 0%」完整可见，点「全书 0%」→ 输入 50 → 落到第 477 / 952 页。

### 内存实测（release，952 页 / 31 MB 真文字 PDF，1440px 无损 WebP）

| 指标 | 结果 |
| --- | --- |
| Rust 进程常驻 | 154 ~ 178 MB（16 页 LRU 上下浮动，**不随翻页增长**） |
| WebView2 进程树 | 开书 606 MB → 翻约 120 页后稳定在 **~950 MB，不再增长** |

结论：AGENTS 里「内存不随翻页无限涨（`loading="lazy"`）」**属实**——但天花板不低，其中约 350 MB 是已被访问过的页位图（Chromium 自己封顶；跳回首末页只回落约 5%）。要压这个天花板，得让远离视口的行不再挂着 `<img>`（占位保留、图按视口 ±N 行挂载），属渲染管线改动，**本次没动**，留作后续决定。

## 附：PDF 能不能指定字体？（2026-09-15 实测 → **结论：不做**）

**结论：PDF 文件里的字形不能被阅读器设置覆盖，但"没嵌字体的 PDF"可以喂替代字体——而且我们已经验证有效。**

- 为什么不能像 EPUB 那样套字体：PDF 是固定版式，`hayro` 把字形栅格化进 PNG，往 HTML 里注入 `font-family` 毫无意义（一期已在顶栏隐藏字号/字体面板）。
- 唯一的口子是 `InterpreterSettings::font_resolver`。`FontQuery::Fallback`（= 未嵌入字体）**回调会被调用、返回的字体数据真的会被使用**（`cid.rs` 里 `font_resolver(&query)` 之后直接建 `OpenTypeFontBlob`），映射链是 `code → ToUnicode → Unicode → 替代字体的 cmap`。
- **实验**（合成一本 `Type0 / Identity-H + ToUnicode + SimSun 不嵌入`、正文 `<0001 0002 0003>` 映射到 一二三）：
  - 默认（`embed-fonts` 硬套标准 14 字体）→ 画出 **`A Æ Á`**：拉丁垃圾，不是空白（这一点比我早先的合成样本更糟，也更隐蔽）；
  - `--substitute-font C:\Windows\Fonts\simsun.ttc` → 正确画出 **`一 二 三`**（宋体笔锋）；
  - `--substitute-font C:\Windows\Fonts\msyh.ttc` → 正确画出 **`一 二 三`**（雅黑无衬线）。
  - 反例（早先的 canary）：同样的字体但**没有 ToUnicode** → 整页空白，替代字体也救不了。
- 入口已就绪：`PdfDoc::render_page_png_with(index, width, Some((bytes, ttc_index)))`；诊断开关 `--substitute-font <path>[#index]`。
- **要不要做成设置项**：可以做，形态是「PDF 字体兜底」而不是「覆盖正文字体」——`FallbackFontQuery` 带 `post_script_name` / `is_serif` / `is_fixed_pitch` / `is_bold` / `is_italic`，足以「按字体名去系统字体里找（SimSun→simsun.ttc），找不到再用用户指定的兜底字体」。
  - 价值取决于书源：我们手上三本样本都不需要（一本嵌入字体、一本扫描+OCR 隐藏层、一本纯扫描）。它救的是「**可见文字 + 未嵌入 + 有 ToUnicode**」那类中文 PDF。
  - 顺带的好处：这类书现在会被 `visible_text_risk` 报「可能缺字」，而默认渲染其实是画出**拉丁垃圾**——有了兜底字体，提示可以升级成「已用系统宋体代替」。
  - 成本：一个 `fontdb` 依赖（或读 `C:\Windows\Fonts` 建索引）+ 设置项 + 每页渲染带上 resolver；不做则维持现状（检测 + 提示）。

### spike 期发现（2026-09-15，已改变 R1 的性质）

`hayro_interpret::font::FontQuery` 只有两个分支，回调（`InterpreterSettings::font_resolver`）**只对标准 14 字体生效**：

```
pub enum FontQuery {
    Standard(StandardFont),      // Times/Courier/Helvetica/Symbol/ZapfDingBats
    Fallback(FallbackFontQuery), // 「not embedded in the PDF file」
}
```

`Fallback` 的官方注释是 *"Note that this type of query is currently not supported, but will be implemented in the future."* —— 也就是说**「未嵌入的非标准字体」（中文 PDF 依赖系统宋体/黑体正是这一类）目前不是「我们写个 resolver 就能补」**，hayro 还不会为它调用回调。

后果与对策（写进风险表 R1）：这类 PDF 用 hayro 渲染会缺字/空白，只能等上游实现、或换 pdf.js / pdfium。因此 spike 的重点从「写 resolver」变成**先量清楚真实样本里有多少这种 PDF**：spike 的体检功能会逐本列出「用到的字体 + 是否嵌入」，比肉眼看图更早、更准地给出结论。扫描版 PDF（整页图片）不受影响，hayro 直接能出图。

## 目标

在现有书架里像 EPUB 一样打开 `.pdf`：能读、能翻页、能记进度、有目录、有封面、能走「编辑元数据…」改名。**不新增发布件形态**（仍然单 exe），不新增浏览器端解析。

非目标（本期）：

- PDF 的划线 / 备注 / 全书% 之外的标注能力、搜索：第二、三期（见「分期」）。
- PDF 阅读皮肤（颜色、字体族、字号覆盖）：与 EPUB 同规律，**不注入**。PDF 的「字号」在语义上是**缩放**。
- 加密 / 带密码 PDF：hayro-syntax 与 hayro 均不支持解密，打开即报错（文案：`加密 PDF 暂不支持`）。这是明确的一期缺口。
- 主题 / 暗色：本软件不做。

## 已拍板决策

1. **引擎 = 纯 Rust 栅格化（hayro）**：PDF 页在 Rust 里渲染成位图，按「一页 = 一个 spine 单元」喂进现有 HTML/iframe 管线。单 exe 不变、iframe 仍不开脚本、「解析不进 JS」禁则不动。
2. **第一期只读**：翻页 + 进度 + 目录 + 缩放（适应页面 / 适应宽度）+ 封面 + 元数据改名。**文字选择、划线、搜索不在第一期**（需要文字层，见第二期）。
3. **存储格式全部沿用现有文件**：`progress.json`（`Locator`）、`annotations.json`、`data/library/<stem>.md`、`<stem>.notes.md`。不为 PDF 另立一套。
4. **书架混排**：`data/library/` 里 `.epub` 与 `.pdf` 同一个书架、同一套排序与三点菜单。
5. **一页 = 一个 spine 单元**（见架构），这样目录、章节跳转、`notes.md` 章节标题、全书% 全部复用现有机制。

## 为什么不是 B / C（决策留痕）

| 方案 | 能拿到什么 | 代价 | 结论 |
| --- | --- | --- | --- |
| **A. hayro（选定）** | 纯 Rust、MIT/Apache-2.0、无 unsafe、CPU 光栅，typst 在用；单 exe；不动禁则 | 无文字层 → 一期不能选择/划线；缩放要重栅格；CPU 渲染成本 | 一期 |
| B. 前端 pdf.js | 现成文本层：选择、搜索、矢量缩放、渲染质量最好 | 渲染进 JS（要放宽「解析不进 JS」）、新增非 iframe 阅读面、CSP 加 `worker-src` | 留作后续（一期渲染质量不达标时的升级路） |
| C. pdfium（DLL） | 兼容性最好，Rust 侧能出字框做文本层 | bblanchon/pdfium-binaries 只发**动态库**（win-x64 = `pdfium.dll` + 导入库，无静态库，静态要自建）；发布件从单 exe 变 exe+dll | 留作后续 |

> 一期**必须先 spike**（见「风险与验证顺序」）：若 hayro 对中文/非嵌入字体或复杂版式的实际输出不能看，就回到 B/C，不要硬推。

## 架构

### 1. 一页 = 一个 spine 单元

新增 `crates/formats-pdf`，实现 `Book` + `BookOpener`（`format_id = "pdf"`）：

| 项 | 取值 |
| --- | --- |
| `spine()` | 每页一条：`id = "p{n}"`，`href = "pdf/page/{n:04}"`（n 从 1 起），`media_type = "application/pdf-page"`，`title = 覆盖该页的目录条目`（无则 `None`） |
| `toc()` | PDF outline 树；无 outline 时按「页序」策略（见待确认 A） |
| `chapter_html(href)` | 极简 HTML，只有一个整页 `<img src="{base}page/w{width}/{n:04}.png">`（`{base}` = `http://icedreader.localhost/book/{id}/`）。不写颜色/字体/`max-width`；iframe 仍 `about:srcdoc`、仍无脚本 |
| `resource(href)` | 解析 `page/w{width}/{n:04}.png` → hayro 渲染 → PNG 字节 |
| `metadata()` | 标题/作者/主题来自 PDF Info + XMP（hayro-syntax `metadata` 模块）；`identifiers` 一期留空（→ 进度键自然落到 `lib:书名.pdf`）；`cover_href` 留空，封面走渲染页 1（见下） |

为什么不是「整本 PDF = 一个 spine 单元（把 N 页全塞进一个 HTML 的多栏）」：那样 3000 页会变成 3000 栏的超宽文档、目录没有可跳的 `href`、`notes.md` 的「第 N 章」也没了。一页一单元让 EPUB 侧全部机制原样可用。

### 2. 渲染与缓存

- 渲染入口：`hayro::render`（`RenderSettings` 给目标尺寸/缩放），输入页来自 `hayro-syntax`。渲染宽度按**桶**取整（720 / 1080 / 1440 / 2160 / 2880），避免每个像素宽度一份缓存。
- 缓存：内存 LRU（最近 N 页，N 取 8–12，注意单页 PNG 数百 KB～数 MB）× 磁盘缓存 `data/pdf-cache/<fileRev>/w{width}/{n:04}.png`（`fileRev` 就是现有 `library::file_rev` 的 `{len}-{mtime}`，文件一换缓存全失效）。磁盘缓存是纯加速，删了自动重建；`delete_book` 顺带清该书的缓存目录。
- **不要阻塞 UI 线程**：`get_chapter`（Tauri 命令）负责触发当前页渲染并可预取相邻页；协议 handler 只从缓存取，未命中时给一个「渲染中」占位（或同步渲染但仅限首页）。具体做法在 spike 里定（关键指标：单页渲染耗时、能否后台线程 + 条件变量等待）。
- 页面旋转（`/Rotate`）、`/UserUnit`、非零页面原点等要在渲染时按 hayro-syntax 暴露的页信息处理（spike 逐项确认）。

### 3. 键 / 元数据 / 封面 / 书库

- **进度键**：`crates/core/src/progress.rs` 目前把 `.epub` 写死在三处（`lib_book_stem`、`same_book` 的语义、`set_book_meta` 里的 `lib:{stem}.epub`）。改为**按已知书籍扩展名剥后缀**（`.epub` / `.pdf`），`-N` 副本规则不变。`progress_key()` 本身已经与扩展名无关（`lib:{相对路径}`）。
- **伴生 md / 改名**：`src-tauri/src/library.rs` 的 `epub_stem` / `rename_book_files` / `unique_stem_ignoring` / `stem_taken` 要按**实际扩展名**工作（`.pdf` 也要进冲突集合与改名）。`set_book_meta` 的键迁移由 `lib:...epub` 改成 `lib:{stem}{ext}`。
- **封面**：`library::cover_bytes` 分派——PDF 渲染页 1（宽度 ~400px）当封面，`BookProfile.has_cover = true`（`metadata.cover_href` 保持 `None`，UI 只看 `hasCover` + `coverRev`）。`/library-cover/<name>` 协议不用改。
- **质量角标 / 同书提示**：`book_signals.rs` 整套是 EPUB 语义（正文字符、标题序列、图片清单、OPF identifier）。PDF 一期**不写 signals**（`open_book` 里按格式早退，`list_library` 自然显示「未知质量」，`duplicates` 为空）。二期可加 PDF 版信号（有无文字层、页数、目录条数）。
- **opener 注册表**：`src-tauri` 现在到处 `EpubOpener` 硬编码（`open_book` / `profile_book` / `cover_bytes` / `reread_book_meta`）。抽一个 `openers()` + `opener_for(path)` + `open_any(path)`，全部改走分派。`library::read_epub_paths` → `read_book_paths`（`.epub` | `.pdf`）。
- **导入**：`portable::import_book_to` 兜底名写死 `book.epub`，应保留源扩展名。`.pdf` 同样「同名复用、不另存 `-2`」。

### 4. 前端

- 「打开 EPUB」→「打开电子书」，dialog 过滤器加 `pdf`（`ui/src/App.tsx` 的 `openEpub`）。
- `OpenedBook.format`（已存在、前端目前没用）作为分支依据：`format === "pdf"` 时
  - 隐藏字号 A± 与字体面板（对 PDF 无意义），显示**缩放：适应页面 / 适应宽度**；
  - 顶栏中部显示「第 N / M 页」而不是「第 x / y 章」；
  - 「划线」入口与划线面板一期禁用/隐藏（二期开启）。
- `ChapterFrame`：加 `pdfMode`（或按 `format` 传入的布局模式）——**此方案已被 SumatraPDF 式连续纸带取代**，作为当时的草案保留：
  - **适应页面**：不注入 `#iced-reader-flow` 分栏，注入 PDF 页样式（整页 `object-fit: contain` 居中、四周留纸色边），一页一屏；
  - **适应宽度**：页宽 = 视口宽、高度溢出 → 该模式下正文容器纵向可滚（滚轮滚页内，左右键仍翻页）；
  - `onProgress` 恒回 0（一页 = 一章，页内无比例）。
- 进度恢复、章边界续翻、目录跳转、全书% 跳转（`chapterChars` 给 `[1; pages]`）**不需要改**。

### 5. 一期明确不做（写进 README 的已知限制）

文字选择、划线/备注、搜索、任意百分比缩放、加密 PDF、PDF 表单/注释层、双页跨页视图（横屏两栏对 PDF 无意义，一期强制单栏）。

## 分期

### 第一期（目标 v0.11.0）只读

1. ~~spike~~ **已完成（2026-09-15，见下）**：三本真实样本 0 本有缺字风险，渲染质量与耗时均可接受 → 可以开第一期。
2. `crates/formats-pdf`：`PdfOpener` / `PdfBook` / 页渲染 + **内存 LRU 缓存（当前页 ±2，不做全书磁盘缓存）** / Info 元数据 / outline→TOC（结构信息用 lopdf：`get_pages`、`get_toc`、页码标签；渲染只用 hayro。理由：hayro-syntax 明确不暴露 outline，自走 `/Outlines` 要处理命名目的地与动作类型，不划算）+ **导入检查 `visible_text_risk`**（命中则在书架上标注「可能显示不全」）。
3. `crates/core`：`PDF_FORMAT` 常量；`progress.rs` 扩展名泛化（+ 单测覆盖 `lib:书名.pdf` / `-N` / 改名迁移）。
4. `src-tauri`：opener 分派、书库扫描/改名/封面/元数据按扩展名泛化、PDF 跳过 signals、`get_chapter` 不做字体注入。
5. `ui`：dialog 过滤、`format` 分支（缩放控件、页数文案、隐藏无关面板）；PDF 阅读面最终由 `ui/src/PdfView.tsx` 承担（连续纸带），`ChapterFrame` 只服务 EPUB。
6. 文档：README（能做什么 / 技术栈 / 目录结构 / 已知限制）、AGENTS.md（分层表加 `crates/formats-pdf`、合法注入清单加 PDF 页样式、书库与进度键的扩展名规则、验证清单）。
7. `.gitignore`：加 `*.pdf` + `!fixtures/*.pdf`（样书约定与 EPUB 一致）。
8. 测试：`crates/formats-pdf` 内部生成最小 PDF（照 `formats-epub` 的 `write_min_epub` 套路，手写最小 PDF 字节即可）；`cargo test -p iced-reader-pdf`、`-p iced-reader-core`、`-p iced-reader`、`npx tsc --noEmit`。

### 第二期：文字层 → 划线 / 备注 / 选择

一行字要能划，必须先有**带坐标的文字层**（现有划线体系锚定在「章内文本节点序号 + 节点内偏移」，见 `ui/src/highlights.ts`）。spike 之后这条路更清楚了：

- **B1（推荐）从 PDF 自己的文字算子生成文字层**：我们已经在用 `lopdf` 解内容流（`page_content_stats` 就是这么做的）。按 `Tf/Td/TD/Tm/TJ/Tj` 取出每一段文字的位置与字号，生成绝对定位的透明 `<span>` 文本层叠在页图上。它同时覆盖两类书：
  - **真文字 PDF**（Windows Everywhere）：文字直接来自内容流；
  - **扫描 + OCR 文字层**（經濟漩渦）：`Tr=3` 的隐藏文字层本来就有坐标，直接搬出来就得到可划可搜的文字层——比重新 OCR 省钱得多。
  - 好处：iframe 仍无脚本、仍 `srcdoc`；`collectTexts` / `anchorFromRange` / `paintHighlights` 原样可用；顺带拿到每页字符数，`chapterChars` 从 `[1; pages]` 升级为真实权重，搜索也有基础。
- **B2 走 hayro 的 `Device`**：在光栅化时收集字形变换。只有 B1 在竖排/复杂排版上不够用时才需要（trait 是给这种用途留的，`hayro-svg` 是参照实现）。
- **B3 换引擎**（pdf.js / pdfium）：spike 已证明不需要。
- **B0 矢量 SVG：已实测否决（2026-09-15）**。`hayro-svg` 能把页面转成 SVG，但实测三本样本：
  | 样本 | SVG 体积 | 内容 |
  | --- | --- | --- |
  | Windows Everywhere p400（文字版） | **4487 KB** | `<text=0` + `<path=3045` → **字形转成轮廓路径** |
  | 經濟漩渦 p100（扫描+OCR） | 369 KB | 1 个 base64 `<image>` |
  | 语文开窍 p200（纯扫描） | 220 KB | 1 个 base64 `<image>` |

  两条结论：① **文字没了**（`<text=0`）——矢量只买到"无限缩放清晰"，换不来选中/搜索；② 文字页 SVG 是我们现在 1440px WebP（224 KB）的 **20 倍**，扫描页也没有更小。所以"文字版 PDF 显示成文字"只能靠 B1 的定位文字层，不能靠矢量。探测工具：`cargo run --release -p iced-reader-pdf --example svg_probe -- <file.pdf> [page]`（`hayro-svg` 只在 dev-dependencies）。
- **顺带记住**：把 PDF 文字抽出来重排成 EPUB 式 HTML 是**错的**——PDF 的版式（页眉页脚、脚注、表格、图文混排）就是内容本身；业界做法也是"位图 + 定位文字层"。

**纯扫描、无文字层**（语文开窍这类，`text_ops=0`）→ 永远没有文字可划/可搜；那张纸只能做「区域划线」（存页矩形，不是文本锚点）。二期可作可选补充，不进一期。判据现成：`page_content_stats().text_ops`。

### 第三期

页内/全书搜索、区域划线、任意百分比缩放、PDF 版质量信号、双页跨页。

## 依赖与许可证

| crate | 版本 | 许可证 | 用途 |
| --- | --- | --- | --- |
| `hayro` | 0.7.x | Apache-2.0 OR MIT | 页栅格化（CPU，纯 Rust，无 unsafe） |
| `hayro-syntax` | 0.7.x | Apache-2.0 OR MIT | 打开 PDF、页信息、Info/XMP 元数据（`metadata` 模块），拿 `PdfData` 给 hayro |
| `lopdf` | 0.45.x | MIT | 页数 / outline / 页码标签等结构信息 |

- 不引入任何 C 依赖 / DLL，**发布件仍是单 exe**。
- 体积：hayro（vello_cpu + skrifa + image 等）预计给 exe 增加数 MB；发布时量一下并记进 release notes。
- `hayro` / `hayro-syntax` 的 `unsafe` feature 是性能开关（默认关）：spike 时对比开/关的渲染耗时再定。

## 风险与验证顺序

| # | 风险 | 验证方式 | 不达标时的退路 |
| --- | --- | --- | --- |
| R1 | **可见文字 + 未嵌入的非标准字体** → 缺字/空白（hayro 的 `FontQuery::Fallback` 上游未实现，见「spike 期发现」） | **已实测**：三本样本 0 本命中；判据已实现为 `visible_text_risk()`（抽样页算子普查 + `Tr` 可见性），进一期做导入检查并在 UI 提示 | 命中时提示「此 PDF 用系统字体，可能显示不全」；占比高再换 C(pdfium) / B(pdf.js) |
| R2 | 复杂版式渲染质量（透明度、混合、渐变、CID 字体） | **已实测**：英文文字页、二值扫描页、彩色封面均正确 | 同上 |
| R3 | 渲染耗时影响翻页手感 | **已实测**：release 1440px 正文页 20~103 ms、封面 385~403 ms | 预取 ±1 页 + 内存 LRU；必要时降到 1080px 栅格 |
| R4 | 加密 PDF 打不开 | 明确报错文案，写进已知限制 | —— |
| R5 | 协议 handler 里同步渲染阻塞 UI | 一期就先量：协议线程被阻塞时窗口是否卡（**spike 阶段无法量**） | `get_chapter` 预渲染 + 协议只取缓存 |
| R6 | 大 PDF 内存（lopdf + hayro 各持一份对象图） | 用 >100MB PDF 试 | 结构信息与渲染分两次读文件 / 二期改共享 |
| R7 | **PNG 体积拖垮缓存**：1440px 单页 0.19~4.4 MB | **已实测** | 只做内存 LRU（当前页 ±2）；不做全书磁盘缓存 |

**spike 顺序**：① `cargo add hayro hayro-syntax lopdf` 能编过（MSVC）✅；② 体检 + 渲染真实样本，看字体与画面（R1/R2）✅ 0 本有风险；③ 计时（R3）与体积（R7）✅；④ 剩余：R5/R6 要到 Tauri 里才能量 → **可以开第一期**。

**spike 脚手架（2026-09-15 已在仓库落地）**：

- `crates/formats-pdf`（工作区成员，暂不含 `Book` 实现）：`PdfDoc::open` / `page_count` / `info` / `outline` / `fonts` / `unresolved_fonts` / `embedded_font_objects`（全书字体程序扫描）/ `page_content_stats`（单页算子普查：文字/路径/图片/Form XObject 递归、`Tr` 可见性、图片字节）/ `render_page_png(index, target_width)` + 计时 + ink 占比。
- `cargo run -p iced-reader-pdf --example render -- <file.pdf|dir> [--width 1440] [--pages 1-3] [--out DIR] [--fonts-only] [--dump-page N]`：单文件出完整报告（含 `visible_text_risk` 终判）；**给目录就逐本体检 + 汇总**；`--pages` 才渲染并写出 PNG 供肉眼比对；`--dump-page N` 解释某一页到底画了什么。
- 单测里用**程序拼装的最小 PDF**（自带合法 xref，不依赖外部样本）验证「能打开、能渲染出字、能读出字体报告」；另有一本「Type0 + 宋体不嵌入 + 可见文字」的合成 PDF 作 R1 复现样例（ignored canary 测试，实测 ink=0）。

## spike 结果（2026-09-15，三本真实样本 + 合成样本）

用户提供的三本样本（都不入库）：

| 样本 | 页数 | outline | 结构 | 结论 |
| --- | --- | --- | --- | --- |
| `Windows Everywhere - Paul Thurrott.pdf`（31.4 MB） | 952 | 148 条 | 真文字 + 6 个嵌入子集字体（OpenSans/LinLibertine/AnonymousPro，`FontFile2`） | ✅ 渲染优秀（正文、斜体、连字、页眉页脚都对），文字可提取 |
| `經濟漩渦.pdf`（19.3 MB） | 570 | 无 | **每页 = 约 29 KB 二值扫描图 + `Tr=3` 隐藏 OCR 文字层**；全文档 0 个字体程序 | ✅ 渲染正确（繁体中文清晰）；文字**可提取**（文字层带 ToUnicode） |
| `语文开窍…pdf`（12.0 MB） | 406 | 71 条 | **纯扫描**：只有整页图片，`text_ops=0`，无文字层 | ✅ 渲染正确；**永远没有文字可划/可搜**（除非外部 OCR） |

**R1（未嵌入字体）在样本里没有成灾，而且我最初的告警是误报**：`經濟漩渦` 的 14 个「未嵌入非标准字体」全是 OCR 文字层用的（`Tr=3` 不可见 + 带 ToUnicode），可见内容其实是位图，不需要任何字形。合成一本「Type0/Identity-H + SimSun 不嵌入 + **可见**文字」的 PDF 仍然实测 **ink=0.00000 整页全白**（hayro `font/type1.rs:33` 的 `// TODO: Actually use fallback fonts`：按 serif/bold/italic 硬套标准字体）。所以正确的判据不是「有没有未嵌入字体」，而是：

> **只有「可见文字（`Tr≠3/7`）用了未嵌入且非标准的字体」才会缺字。** 隐藏文字层与纯扫描都无害。

这个判据已实现为 `visible_text_risk()`（抽样最多 24 页做算子普查），三本样本判定 **0 本有风险**；它也是二期「这本书能不能划线/搜索」的信号来源。

其它已确认（绿灯）：

- **编译**：`hayro` 0.7.1 / `hayro-interpret` 0.7.0 / `hayro-syntax` 0.7.2 / `vello_cpu` 0.0.8 / `lopdf` 0.45 全在 Windows MSVC（rustc 1.98.1）编过，**零 C 依赖 / 零 DLL**，仍单 exe。
- **体积（已实测）**：接入后 `target/release/IcedReader.exe` = **16.98 MB**（接入前 11.56 MB，**+5.4 MB**）。示例 exe（只有 hayro + lopdf + image + 本 crate）release = 7.05 MB，可见增量主要是栅格化栈。若要压体积，可考虑 release profile 开 `strip`/LTO（未做，留给发布时决定）。
- **`embed-fonts` 有效**：未嵌入的标准 14 字体（Helvetica 等）照常出字，所以「未嵌入」本身不是问题。
- **结构信息**：`lopdf` 一次读入即得页数、Info（Title/Author）、outline（扁平 `{level, title, page}`，自己按 level 还原成树）；无 outline 时 `get_toc()` 返 `NoOutline` → 按已拍板返回空目录。
- **封面缩略图**：第 1 页按 400px 宽渲染 = 18~455 ms、165~453 KB PNG（首本较慢是页 1 为大图封面，解码占主导）。

**R3 实测（release，1440px 宽）—— 足以支撑「翻页不用预渲染」**：

| 页型 | 渲染耗时 | PNG |
| --- | --- | --- |
| 英文文字页（嵌入字体） | 20.6 ~ 24 ms | 0.76 ~ 1.15 MB |
| 扫描页（二值 + 隐藏文字层） | 55 ~ 103 ms | 0.19 ~ 1.09 MB |
| 彩色整页封面 | 385 ~ 403 ms | 1.9 ~ 4.4 MB |

由此定下两条**缓存结论**：① **只做内存 LRU（当前页 ±2 页，约 10~20 MB），不做全书磁盘缓存**——570 页 × 约 1 MB/页 ≈ 570 MB，磁盘缓存不划算，而 20~100 ms 的渲染延迟靠预取即可掩盖；② 降采样到 1440px 已足够清晰（样本在 1440px 下肉眼无损失），2880px 只在放大时才需要。

**仍未实测**：加密 PDF（R4，已知 hayro 不支持解密）、协议线程同步渲染的卡顿（R5，要进 Tauri 后才能量）、>100 MB 单文件内存（R6）。

## 待确认

- ~~A. 无 outline 时的目录策略~~ **已拍板（2026-09-15）：无 outline 就不给目录**——侧栏空着/提示「本书没有目录」，靠翻页、进度与全书%导航，不做页序长列表。一期 `toc()` 直接返回空树。
- **B. 缩放控件形态**：顶栏常显两个图标，还是收进「⋯」菜单；是否记忆（存 `settings.json`）。
- **C. 进度键是否用 PDF 的 trailer `/ID`**：用了则改名/移动后仍稳（对齐 EPUB 的 `id:` 语义，代价是同一文件的两个副本共享进度——与 EPUB 一致）。一期倾向不用，走 `lib:`。
- ~~D. 样本 PDF~~ **已到位（2026-09-15，三本放仓库根、不入库）**：英文文字版（带 148 条 outline）、中文扫描 + OCR 文字层（无 outline）、中文纯扫描（71 条 outline）。结论见上表。
- **E. PDF 的「章标题」在 `notes.md`**：一期无划线，二期随文字层一起定（outline 标题 / 「第 N 页」）。
- ~~F. R1 的退路~~ **已定（2026-09-15）：继续用 A（hayro），不换引擎**。样本 0 本命中；把 `visible_text_risk` 做成导入时的检查 + 命中时在书架/阅读器提示「此 PDF 依赖系统字体，部分页可能显示不全」。等真遇到大面积缺字再评估 C/B。

## 验证（一期手工清单）

- 书架：`.pdf` 出现在书架、有封面、能打开；与 EPUB 混排排序正常；坏 PDF 显示「无法打开」且能删。
- 阅读：翻页（左右键 / 滚轮）逐页前进；章边界续翻；首/末页边界不越界；适应页面 / 适应宽度切换正确；横屏仍单栏。
- 进度：退出重进回到同一页；书架进度条、章节号（第 N 页）正确；改元数据改名后进度与封面不丢。
- 目录：有 outline 的书能跳页并高亮当前条；无 outline 的书侧栏显示「本书没有目录」，不崩不空指针。
- 全书%：输入 50 跳到整本中间那一页。
- 回归：EPUB 全链路（打开、分页、划线、词注、字体、封面）无变化；`cargo test -p iced-reader-core` / `-p iced-reader-epub` / `-p iced-reader`、`npx tsc --noEmit` 全过。
