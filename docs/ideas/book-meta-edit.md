# 想法/进展：书名规范 = 编辑元数据 + 伴生 md

状态（2026-09-04 起稿，多提交推进）：v1 核心+UI 已落地；md v2 字段/拼接模板/译者/保存即改名已落地；**作者多名分隔改 ASCII 半角逗号（书名不出现中文标点）+ 面板「重新读取原书元数据」在本提交落地**；全部功能已随 v0.9.0（2026-09-04）发布（见 `docs/releases/v0.9.0.md`）。剩余：真实脏书名样例与清洗规则扩充（用户后补）、桌面窗口手工核对、作者行联动/年份自动读取等可选增强。

## 目标

把模糊的「书名规范」收敛为：给书架条目加一层**用户可编辑的元数据**，显示名裁决链为 `手填显示名 → 字段拼接 → dc:title → 文件名`；元数据存成与 epub 同名的伴生 Markdown（程序维护，用户只走 UI，不手编 md）。初期**不做**自动抓取（豆瓣/亚马逊等），用户后补真实脏样例再扩充清洗规则。

## 已拍板决策

1. **入口**：书架封面右下三点菜单加「编辑元数据…」，置于「从书库删除」上方；面板标题可用「书名规范 / 编辑书籍信息」。
2. **字段宁少勿多 → v2 扩展**：初版 `title` / `subtitle` / `volume` / `displayTitle`；2026-09-04 扩展为 `title` / `subtitle` / `volume` / `author` / `year` / `publisher` / `isbn` / `displayTitle`；随后加 `translator`（译者，拼入标题）。原书名/原副标题/原ISBN **不再拆**（用户 2026-09-04 明确「没尽头了」）：`originalTitle` 只读保留第一次见到的完整书名，英文原名等属书名一部分留在 `title` 字段由用户自行取舍。
3. **md 由程序维护**：不是用户手写格式；用户编辑一律走 UI 面板。md 人类可读只是可拷贝/备份/进 git 的福利。
4. **md v1 不含划线**：划线仍存 `data/annotations.json`；将来「md 存划线 / 导出笔记」另起版本。
5. **裁决链（写进 AGENTS）**：用户确认过的 `displayTitle` → 字段自动拼接 → `dc:title`（非空且非 "Untitled"）→ 文件名兜底。手改的显示名永不被子段自动拼接覆盖。书架、顶栏、排序、删除确认共用同一结果，不分裂。
6. **originalTitle**：首次导入时程序见到的书名。存量书（功能上线前已入库）取功能上线后**第一次保存时**程序见到的书名。
7. **删除书连带删 `<stem>.md`**。
8. **符号约定（全角禁则，v2 调整）**：
   - 程序生成的符号一律 ASCII，**绝不产出全角**；原书 `dc:title` 自带的字符（含 `Ⅲ`、全角冒号等）原样保留，不转半角（那是书名的一部分）。
   - **拼接模板**（用户 2026-09-04 拍板）：`书名 [ _ 副标题] [ - 卷册] [ - 作者] [ - 译者] [ - 出版年份] [ - 出版社] [ - ISBN]`。下划线 `" _ "` **只出现在书名与副标题之间**；卷册起一律用字段级分隔 `" - "`。空字段整体跳过，绝不出现两分隔符夹空段；书名必填，留空不拼接。
   - ISBN 填号码，拼入时自动补 ASCII `ISBN ` 前缀（值已以 ISBN 开头则保留原样）；译者填姓名，拼入时自动补「译者 」标签（值已以「译者」开头则保留原样，不用全角冒号）。
   - **作者/译者多名分隔禁顿号**（用户 2026-09-04：`、` 违反约定，书名中不应出现中文标点）：预填与保存/拼接时 U+3001 一律折成 ASCII `, `（`clean_person_list`）。`dc:title` 自带字符（含全角冒号）不在此范围，仍原样保留。
9. **保存即按显示名改名**（用户 2026-09-04 拍板「书籍/md文件名应按保存后的拼接文件名；显示名到 md 里取」）：`set_book_meta` 成功后把 `data/library/` 里的 epub + 伴生 md 按“保存后的显示名（手改或拼接）”改名（Windows 禁用作清洗、同名冲突 `-2`/`-3`…）；有 `id:` 键（有 identifier）的书进度/划线不受影响，`lib:` 键的书把进度/划线/质量信号缓存迁到新键；改名失败整次保存报错。
10. **重新读取原书元数据**（用户 2026-09-04）：面板原书名区旁有按钮，清空所有手填（含显示名）后重新打开 epub，用原书 `dc:title`（清洗空白）/`dc:creator`（多名 ASCII 逗号）/`dc:publisher`/identifier 里的 ISBN 填表；副标题/卷册/译者/年份无原书来源置空；originalTitle 定格不动；**不自动保存**，是否保存由用户决定。
11. **译者后缀 + 原文书名写括号**（用户 2026-09-30 拍板）：译者段由「译者 」前缀改为结尾「译」，即 `… - 宋文伟 译 - …`（`TRANSLATOR_LABEL` → `TRANSLATOR_SUFFIX`，值已以「译」结尾则原样、不重复）；**原文书名不单列字段**，用户自己在主书名/副标题里写括号（例：`性政治 (Sexual Politics)`），括号原样进显示名与文件名（`clean_file_stem` 不替换括号），面板主书名/副标题下各有一行说明。决策 8 的「译者 」标签写法随本次作废，保留原文作记录。
12. **伴生文件合并为一个 + 去掉手填显示名**（用户 2026-09-30 拍板，破坏性：旧文件一律当不存在，可删库重来）：
    - 一本 `X.epub` / `X.pdf` 只有**一个**伴生 md：`data/library/X.epub.md`（**文件名.扩展名.md**，扩展名是身份的一部分，同名 epub/pdf 不再互撞）。旧 `<stem>.md` 与 `<stem>.notes.md` 不再读、不迁移、不删。
    - 该文件同时装 `<!-- icedreader-meta -->` 元数据块与 `<!-- icedreader-note -->` 划线块（块的机器字段补上渲染高亮必需的 `href`/`startText`/`startOffset`/`endText`/`endOffset` 与 `text`）。两侧各管各的块，其余（含文件头散文与用户笔记区）逐字保留：`split_meta` / `join_meta`。
    - **`data/annotations.json` 废弃**：划线随书走，删书随之进回收站。解析器从 `src-tauri/src/notes.rs` 下沉到 `crates/core/src/notes.rs`（`core` 不能依赖 `src-tauri`），`src-tauri/src/notes.rs` 只转发。
    - **去掉 `displayTitle`**（md 键与面板输入框一起删）：裁决链只剩 `字段拼接 → dc:title/文件名`，书架显示名与库内文件名恒等，不再有两个名字打架。问题起点是「显示名」留空即自动、填了就锁定，与「文件名跟随拼接结果」冲突。
13. **`bookFile` → `originalBookFile`（首次进书架时写入）+ 新增 `md5`；删 `originalTitle`**（用户 2026-09-30 拍板）：
    - `bookFile` 旧行为是「**首次保存元数据时**写一次」（`set_book_meta` 里 `existing.book_file.or_else(|| 当前名)`），所以 app 内改名后它必然过期——实测：你的 md 里 `bookFile:` 记的是按**当时那套字段**拼出的旧名（`性政治 - (美)凯特·米利特著 - 江苏人民出版社 - ISBN 9787214026088.epub`），而当前文件名是 `性政治 (Sexual Politics) - 凯特·米利特 (Kate Millett) - 宋文伟 译 - 2000 - 江苏人民出版社 - ISBN 9787214026088.epub`。而且**没有任何地方读它**，`Some("")` 还会被永久保留。
    - 改为 `originalBookFile`：**这本书首次进入书架时的库内文件名**，此后永不改（书改名了它仍记原来的名字）。写入时机 = 首次落档（导入 / 打开 / 首次保存元数据，谁先到谁写，幂等），不再是「首次保存」。
    - 新增 `md5`：**首次落档时那个书文件字节的 MD5**（32 hex）。本阅读器从不重写书文件（只改名 / 送回收站），所以「导入的字节 = 永远看到的字节」，md5 就是「这条 md 认的是哪个文件」的证据，将来文件被换掉时用于校验。内容级的 `book_signals::fingerprint` 仍只服务同书对照，两者口径不同、互不替代。
    - 删 `originalTitle`：`originalBookFile` 已留下「它来时叫什么」，而原书当前书名随时可由面板「重新读取原书元数据」从文件里取回；冻结一份反而会与文件不一致（同一本书换了版本/文件后，它记的还是最初那个名字）。面板「原书名（只读）」一行随之删除。
    - 旧 md 的 `bookFile` 当作 `originalBookFile` 读入（那是它能给的最早的名字），`originalTitle` 直接忽略，写回去只写新键名。块是按结构体重写的 —— **旧版本再保存一次会把 `md5` 丢掉**，加字段要记住这点。
    - 每个键的唯一定义（一义一键）写在 `crates/core/src/book_meta.rs` 的模块文档里，代码与文档同址。

## 实现现状（v1 已提交 `e498e8f`；md v2/拼接模板/译者/保存改名已提交；ASCII 分隔与重读在本提交）

- `crates/core/src/book_meta.rs`：`BookMeta` 结构 + `<文件名>.md`（`三体.epub` → `三体.epub.md`）读写（v2 字段 author/translator/year/publisher/isbn；宽容解析、v1 md 读入为空不丢；`split_meta`/`join_meta` 让元数据块与划线区互不碰）；`clean_title`；`clean_person_list`（顿号 U+3001 → ASCII `, `）；`join_title`（拼接模板，作者/译者段过 `clean_person_list`，译者补结尾「译」/ISBN 补 ASCII 前缀）；`resolved_title`；带单测。
- `crates/core/src/lib.rs`：导出 `book_meta`（含 `clean_person_list`）。
- `crates/core/src/progress.rs`：`rename_key`；`crates/core/src/annotations.rs`：`rename_book`（均仅 `lib:` 键）。
- `src-tauri/src/book_meta.rs`：`BookMetaFields`/`BookMetaView` + `view_for`（作者预填原书 dc:creator，多名 ASCII 逗号）；`reread_view_for`（重读原书建视图）+ `extract_isbn`（identifier 里取 ISBN-like 并剥前缀）；带单测。
- `src-tauri/src/lib.rs`：命令 `get_book_meta`/`reread_book_meta`/`set_book_meta`；`set_book_meta` 编排「保存即改名」（改 epub + 伴生 md → `lib:` 进度/质量信号键迁移 + 缓存清理 → 写新 md）。
- `src-tauri/src/library.rs`：`clean_file_stem`/`unique_stem`/`rename_book_files`；`book_signals.rs`：`rename_key`；均带单测。
- `ui/src/BookMetaPanel.tsx`：v2+ 全字段布局 + 主书名必填 + 「重新读取原书元数据」按钮（清空手填/显示名、填充原书字段、不自动保存）+ 预览镜像 join_title + 操作行常驻底部 + 文件改名提示；模板说明在预览框外。
- `AGENTS.md`：书元数据小节（ASCII 逗号分隔、重读命令、保存改名语义）；验证段同步。

**验证状态**：本提交 `cargo test`（core 40 / lib 28 / epub 18+2 ignored）与 `npx tsc --noEmit`、`vite build` 通过。桌面窗口手工核对（重读按钮、改名后书架/封面/进度、-N 冲突）尚未做。

**验证状态**：本提交 `cargo test`（core 37 / lib 23 / epub 18+2 ignored）与 `npx tsc --noEmit`、`vite build` 全部通过。桌面窗口手工核对尚未做（无窗口环境）。

## 已完成（下一步）

1. ✅ **UI 面板**：`Library.tsx` 三点菜单加「编辑元数据…」，模态面板含 title/subtitle/volume 输入、拼接预览与「自动填充」按钮、displayTitle 手改框（自动填充不覆盖手改）、originalTitle 只读展示；保存 → `set_book_meta` → 重新 `list_library` 刷新书架。
   - 交互关键设计：手改框初值 = md 里用户确认过的 displayTitle（未确认过则为空）；空 = 派生模式。`BookMetaView.confirmedTitle`（md 原始值）与 `displayTitle`（裁决结果）分开，避免首次保存把旧名锁死。自动填充仅在手改框为空时可点，把字段拼接写入框内；手改非空即锁定（编辑字段也不会覆盖）。
   - **已作废（2026-09-30）**：手改框与「自动填充」按钮随「去掉手填显示名」一起删除（见「已拍板决策 12」与「已完成 7」），本条只作 v1 交互的记录。
2. ✅ **AGENTS.md 同步**：新增「书元数据（书名规范）」小节（裁决链、伴生 md 约定、全角禁则与 `" _ "`/`" - "` 符号分工、删书联动）；Tauri 命令表补 `get_book_meta`/`set_book_meta`；书架菜单描述与验证段更新。
3. ✅ **验证**：`cargo test`（core 37 / lib 22 / epub 18+2 ignored 全过）、`npx tsc --noEmit` 无错。手工核对（书架/顶栏标题一致、删书连带删 md、面板交互手感）需桌面窗口。
4. ✅ **md v2：字段扩展 + 新拼接模板**（用户 2026-09-04 拍板）：加 `author` / `year` / `publisher` / `isbn`；拼接模板 `书名 _ 副标题 - 卷册 - 作者 - 出版年份 - 出版社 - ISBN`（下划线仅书名↔副标题一处，卷册起全用 ` - `）；空段整体跳过不产生连续分隔符；书名必填（UI 禁保存）；ISBN 自动补 ASCII `ISBN ` 前缀；作者预填原书 dc:creator。
5. ✅ **译者字段 + 保存即改名**（用户 2026-09-04 拍板 C 方案与「书籍/md文件名应按保存后的拼接文件名；显示名到 md 里取」）：加 `translator`，模板插到作者后 `- 译者 阳曦`（自动补标签）；`set_book_meta` 保存成功后把 epub+md 按最终显示名（手改或拼接）改名，Windows 禁作清洗、同名 `-2`…、`lib:` 进度/划线/质量信号键自动迁移、`id:` 书天然不受影响。原书名/原副标题/原ISBN 不拆（用户收止）。
6. ✅ **译者改结尾「译」+ 原文书名写括号**（用户 2026-09-30）：译者段改成 `- 宋文伟 译`（补结尾，不重复）；原文书名不单列字段，改由用户在主书名/副标题里写括号，面板两处各加一行说明（见「已拍板决策 11」）。
7. ✅ **伴生文件合一 + 去掉手填显示名**（用户 2026-09-30，破坏性，见「已拍板决策 12」）——本提交：
   - 一本一书只剩一个伴生 md：`data/library/<文件名>.md`（`三体.epub.md` / `三体.pdf.md`，扩展名是身份的一部分），元数据块与划线块同文件，`split_meta`/`join_meta` 保证两侧写入互不碰；旧的 `<stem>.md` / `<stem>.notes.md` / `annotations.json` 一律当不存在，不迁移也不删。
   - 划线解析/写回从 `src-tauri/src/notes.rs` 下沉到 `crates/core/src/notes.rs`（`core` 不能依赖 `src-tauri`），Tauri 侧只转发路径；`AnnotationStore` 从「`annotations.json` + 内存 HashMap，按 `id:`/`lib:` 键寻址」改为「按**书文件名**寻址伴生 md」的读写门面（`list`/`remove`/`set_pos`）；创建仍只走 `write_highlight_entry`，因为只有它拿到打开的书、能定该条的 `## 第 N 章 · …` 归属。
   - `displayTitle` 从 md 键、`BookMetaFields`、`BookMetaView`（`confirmedTitle`/`displayTitle`/`suggestedTitle` 三个字段并成 `joinedTitle`）与面板（手改框 + 「自动填充」按钮）一起删除；裁决链只剩 `字段拼接 → dc:title/文件名`。
   - 顺带修两处真问题：① `AnnotationStore::set_pos` 改走 `notes::update_pos` —— 位置回填只重写保护区与摘抄行，用户笔记区逐字保留（原走 `upsert` 会把 `stored_highlights` 读回来的 `trim()` 副本写回盘，吃掉用户笔记的行首缩进/行尾空格）；② `set_annotation_pos` 不再声明前端根本没发的 `bookId`（Tauri 对缺失的必填参报 `missing required key`，整次调用失败又被前端 `catch {}` 吞掉 —— 首次开书那批 `pos: null` 划线的位置回填会静默失效）。
   - 面板顺带去掉一处必然重复：没有「显示名」之后，预览框下面那行「保存后书架与文件名都用这个名字：<同一串>」与「拼接预览」恒等（去掉显示名前它显示的是可能不同的生效名），整行删除，改名后果的说明并进预览框（`.meta-effect` 样式随之删除）；主书名留空时不显示这句后果说明。
   - 验证：`cargo test --workspace` 全绿（iced-reader 68 + 3 ignored / core 61 / epub 33 + 2 ignored / pdf 17 + 3 ignored）、`npx tsc --noEmit`、`npm run check:ui` 通过；`set_pos` 的保真修复有回归单测 `set_pos_leaves_the_user_note_byte_for_byte`（换回 `upsert` 会红）。桌面窗口手工核对已做（见下）：划线按章归组、位置回填真的落盘、改名后伴生 md 随书更名、删书置回收站可还原、有备注的划线删除留痕且备注保留。

## AGENTS.md 同步（已完成，原草案存档）

已按以下内容写入 AGENTS.md「书元数据（书名规范）」小节，原草案存档如下（v1 草案；v2 的拼接模板/分隔规则以「已拍板决策 8」与 AGENTS 现文为准）：

- 书架条目增加用户可编辑元数据，存 `data/library/<stem>.md`（与 epub 同名，程序维护，用户走 UI 不手编）。
- 显示名裁决链：md `displayTitle`（用户确认过）→ 字段拼接 → `dc:title`（非空且非 "Untitled"）→ 文件名；`list_library`/`open_book` 统一应用，书架与阅读标题一致。
- 程序生成符号一律 ASCII 禁全角；同性质拼接用 ` _ `、不同性质字段用 ` - `（已确认）；不转写原书自带字符。
- `delete_book` 连带删除伴生 md。
- 划线仍存 `annotations.json`，不进 md（v1）。

## 待补 / 待确认

- 真实脏书名样例（用户后补）——用于扩充清洗规则；当前 `clean_title`/`clean_person_list` 保守（折叠空白、顿号折半角逗号），不猜书名主体（`[美]` 等国籍前缀同样不自动去）。
- 桌面窗口手工核对：重读按钮行为、改名后书架刷新/封面/进度/划线保留、同名冲突 `-N`、长表单滚动、Esc/遮罩关闭、顶栏与书架标题一致、删书连带删 md（AGENTS「验证」已列清单）。
- **未拍板**：书架/顶栏第二行作者目前仍显示原书 dc:creator，而标题已按模板含 md 作者/译者 —— 是否改为 md 作者优先（或作者行消隐）待定。
- 后续可选：出版年份从 OPF `dc:date` 自动读（需 formats-epub 解析并扩展 `Metadata`）；出版社/ISBN 自动预填已随重读按钮提供；译者从 dc:contributor（role=translator）预填；md 承载划线并支持导出；孤儿 md 扫描配对。
- **注意点**：改名冲突产生的 `书名-2.epub` 与 `书名.epub` 在进度键层视为同一本（AGENTS 既有 `-N` 规则）——若将来真出现两本不同书拼接名仅差 `-N`，进度会共享，需专门策略。
