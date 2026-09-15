//! Spike harness: audit + rasterise real PDFs.
//!
//! ```text
//! cargo run -p iced-reader-pdf --example render -- <file.pdf|dir> \
//!     [--width 1440] [--pages 1-3] [--out DIR] [--fonts-only]
//! ```
//!
//! Single file: full report (page count, Info, outline, **font embedding
//! audit**, then per-page render time and PNG size).
//!
//! Directory: one line per `*.pdf` in it, so a folder of samples answers the
//! R1 question ("how many of these depend on system fonts?") in one run;
//! `--pages N` renders that page of every file for eyeballing.
//!
//! PNGs land in `<stem>.spike/` next to the file (or `--out`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use iced_reader_pdf::{
    is_standard_font_name, FontAudit, PageFormat, PdfDoc, RenderOptions,
};

struct Args {
    path: PathBuf,
    width: u32,
    pages: Option<(usize, usize)>,
    out: Option<PathBuf>,
    fonts_only: bool,
    dump_page: Option<usize>,
    /// `--substitute-font <path>[#index]`: font program handed to the renderer
    /// for fonts the PDF does not embed (diagnostic; not a shipped setting).
    substitute: Option<(Vec<u8>, u32)>,
    /// `--format png|jpeg` (default jpeg, like the reader serves).
    format: PageFormat,
}

fn usage() -> ! {
    eprintln!(
        "usage: render <file.pdf|dir> [--width N] [--pages A-B] [--out DIR] [--fonts-only] \
         [--dump-page N] [--substitute-font FILE[#TTC_INDEX]] [--format png|jpeg]"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut path: Option<PathBuf> = None;
    let mut width = 1440u32;
    let mut pages = None;
    let mut out = None;
    let mut fonts_only = false;
    let mut dump_page = None;
    let mut substitute = None;
    let mut format = PageFormat::Png;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--width" => {
                width = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            "--pages" => {
                let value = args.next().unwrap_or_else(|| usage());
                let (a, b) = match value.split_once('-') {
                    Some((a, b)) => (a, b),
                    None => (value.as_str(), value.as_str()),
                };
                let a: usize = a.parse().unwrap_or_else(|_| usage());
                let b: usize = b.parse().unwrap_or_else(|_| usage());
                pages = Some((a.max(1), b.max(a)));
            }
            "--out" => out = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--fonts-only" => fonts_only = true,
            "--dump-page" => {
                dump_page = args.next().and_then(|v| v.parse().ok());
                if dump_page.is_none() {
                    usage();
                }
            }
            "--substitute-font" => {
                let value = args.next().unwrap_or_else(|| usage());
                let (file, index) = match value.rsplit_once('#') {
                    Some((file, index)) => (
                        file.to_string(),
                        index.parse::<u32>().unwrap_or_else(|_| usage()),
                    ),
                    None => (value, 0),
                };
                match std::fs::read(&file) {
                    Ok(bytes) => substitute = Some((bytes, index)),
                    Err(err) => {
                        eprintln!("[!] 读不了替代字体 {file}: {err}");
                        std::process::exit(1);
                    }
                }
            }
            "--format" => {
                format = match args.next().unwrap_or_else(|| usage()).as_str() {
                    "png" => PageFormat::Png,
                    "jpeg" | "jpg" => PageFormat::Jpeg,
                    "webp" => PageFormat::WebP,
                    _ => usage(),
                };
            }
            other => {
                if path.is_none() && !other.starts_with("--") {
                    path = Some(PathBuf::from(other));
                } else {
                    usage();
                }
            }
        }
    }
    Args {
        path: path.unwrap_or_else(|| usage()),
        width,
        pages,
        out,
        fonts_only,
        dump_page,
        substitute,
        format,
    }
}

fn count_outline(nodes: &[iced_reader_pdf::OutlineNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + count_outline(&node.children))
        .sum()
}

fn mb(len: u64) -> f64 {
    len as f64 / (1024.0 * 1024.0)
}

/// Per-file audit summary — everything the R1/R2/R3 verdict needs, minus the
/// pixels.
struct Audit {
    pages: usize,
    info_title: Option<String>,
    outline_top: usize,
    outline_total: usize,
    fonts: Vec<FontAudit>,
    unresolved: Vec<FontAudit>,
    parse_ms: f64,
    size_mb: f64,
}

fn audit(path: &Path) -> Result<(PdfDoc, Audit), String> {
    let doc = PdfDoc::open(path).map_err(|e| e.to_string())?;
    let outline = doc.outline();
    let size_mb = std::fs::metadata(path).map(|m| mb(m.len())).unwrap_or(0.0);
    let audit = Audit {
        pages: doc.page_count(),
        info_title: doc.info().title,
        outline_top: outline.len(),
        outline_total: count_outline(&outline),
        fonts: doc.fonts(),
        unresolved: doc.unresolved_fonts(),
        parse_ms: doc.parse_ms(),
        size_mb,
    };
    Ok((doc, audit))
}

fn font_line(font: &FontAudit) -> String {
    let flag = if font.embedded {
        "embedded"
    } else if is_standard_font_name(&font.name) {
        "standard"
    } else {
        "MISSING "
    };
    format!(
        "  [{flag}] {:<10} {:<28} enc={:<18} toUnicode={:<5} {}",
        font.subtype,
        font.name,
        font.encoding.as_deref().unwrap_or("-"),
        font.to_unicode,
        font.file.as_deref().unwrap_or("-")
    )
}

fn render_pages(doc: &PdfDoc, args: &Args, out_dir: &Path) -> f64 {
    if let Err(err) = std::fs::create_dir_all(out_dir) {
        eprintln!("[!] 建不了输出目录 {}: {err}", out_dir.display());
        return 0.0;
    }
    let (first, last) = match args.pages {
        Some((a, b)) => (a.saturating_sub(1), b.saturating_sub(1)),
        None => (0, 0),
    };
    let last = last.min(doc.page_count().saturating_sub(1));
    let mut slowest = 0.0f64;
    for index in first..=last {
        let opts = RenderOptions {
            width: args.width,
            format: args.format,
            substitute_font: args.substitute.clone(),
        };
        match doc.render_page(index, &opts) {
            Ok(page) => {
                slowest = slowest.max(page.render_ms);
                let name = format!("page-{:04}.{}", index + 1, page.format.extension());
                if let Err(err) = std::fs::write(out_dir.join(&name), &page.data) {
                    eprintln!("[!] 写不了 {name}: {err}");
                }
                println!(
                    "  p{:<5} {}x{}  raster {:>7.1} ms  encode {:>6.1} ms  total {:>7.1} ms  ink {:>6.2}%  {:>4} {:>8.1} KB  {name}",
                    index + 1,
                    page.width,
                    page.height,
                    page.raster_ms,
                    page.encode_ms,
                    page.render_ms,
                    page.ink * 100.0,
                    page.format.extension(),
                    page.data.len() as f64 / 1024.0
                );
            }
            Err(err) => eprintln!("[!] 第 {} 页渲染失败: {err}", index + 1),
        }
    }
    slowest
}

fn spike_dir_for(path: &Path, args: &Args) -> PathBuf {
    args.out.clone().unwrap_or_else(|| {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "pdf".into());
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}.spike"))
    })
}

fn run_single(path: &Path, args: &Args) {
    let t_open = Instant::now();
    let (doc, audit) = match audit(path) {
        Ok(ok) => ok,
        Err(err) => {
            eprintln!("[!] 打不开 {}: {err}", path.display());
            return;
        }
    };
    let open_ms = t_open.elapsed().as_secs_f64() * 1000.0;

    println!("file      : {} ({:.1} MB)", path.display(), audit.size_mb);
    println!("pages     : {}", audit.pages);
    println!(
        "open      : {open_ms:.1} ms (lopdf + hayro parse: {:.1} ms)",
        audit.parse_ms
    );
    println!("title     : {:?}", audit.info_title);
    println!(
        "outline   : {} top-level / {} nodes total{}",
        audit.outline_top,
        audit.outline_total,
        if audit.outline_top == 0 {
            "  -> 无 outline：按约定不给侧栏目录"
        } else {
            ""
        }
    );
    println!(
        "fonts     : {} distinct, {} unresolved",
        audit.fonts.len(),
        audit.unresolved.len()
    );
    for font in &audit.fonts {
        println!("{}", font_line(font));
    }
    if !audit.unresolved.is_empty() {
        let risk = doc.visible_text_risk();
        if risk.is_empty() {
            println!(
                "      -> 这些字体只用于隐藏文字层(Tr=3)/纯扫描页，不影响画面：本书实测渲染正常"
            );
        } else {
            println!(
                "[!] 真实风险：{} 用于**可见**文字且未嵌入 → 这些页会缺字/空白",
                risk.join(", ")
            );
        }
    }

    if let Some(page_number) = args.dump_page {
        let embedded = doc.embedded_font_objects();
        println!(
            "embedded  : {} 个字体程序{}",
            embedded.len(),
            if embedded.is_empty() {
                "  -> 全书无嵌入字体：可见文字只能是轮廓路径(转曲)或图片"
            } else {
                ""
            }
        );
        for font in &embedded {
            println!(
                "  {:<10} {:<30} {:>8.1} KB",
                font.key,
                font.font_name,
                font.bytes as f64 / 1024.0
            );
        }
        match doc.page_content_stats(page_number.saturating_sub(1)) {
            Ok(stats) => {
                println!(
                    "page {page_number:<5}: text_ops={} path_ops={} xobject_ops={} forms={} images={} inline_img={} img_B={:.1}KB Tr={:?}",
                    stats.text_ops,
                    stats.path_ops,
                    stats.xobject_ops,
                    stats.forms,
                    stats.image_objects,
                    stats.inline_images,
                    stats.image_bytes as f64 / 1024.0,
                    stats.text_render_modes
                );
                if stats.text_render_modes.contains(&3) {
                    println!("      -> 文字是 Tr=3 隐藏层（扫描件 OCR 文字层）：看得见的是位图");
                }
                for font in &stats.fonts {
                    println!("{}", font_line(font));
                }
                if stats.text_ops == 0 {
                    println!("      -> 本页没有文字算子：该页文字是转曲/图片，二期无法划线或搜索");
                }
            }
            Err(err) => eprintln!("[!] 第 {page_number} 页内容读不出: {err}"),
        }
        return;
    }

    if args.fonts_only {
        return;
    }
    let out_dir = spike_dir_for(path, args);
    println!("render    : width={} px -> {}", args.width, out_dir.display());
    let slowest = render_pages(&doc, args, &out_dir);
    println!("slowest   : {slowest:.1} ms/page");
}

fn run_directory(dir: &Path, args: &Args) {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(read) => read
            .filter_map(|item| item.ok())
            .map(|item| item.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            })
            .collect(),
        Err(err) => {
            eprintln!("[!] 读不了目录 {}: {err}", dir.display());
            return;
        }
    };
    files.sort();
    if files.is_empty() {
        println!("目录里没有 *.pdf：{}", dir.display());
        return;
    }

    println!("目录 {} 下 {} 个 PDF\n", dir.display(), files.len());
    let mut dirty: Vec<String> = Vec::new();
    let mut fonts_only_mode = args.fonts_only;
    // A folder run is about the audit; rendering is opt-in via --pages.
    if args.pages.is_none() {
        fonts_only_mode = true;
    }
    let per_file = Args {
        path: PathBuf::new(),
        width: args.width,
        pages: args.pages,
        out: args.out.clone(),
        fonts_only: fonts_only_mode,
        dump_page: None,
        substitute: args.substitute.clone(),
        format: args.format,
    };

    for path in &files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match audit(path) {
            Ok((doc, audit)) => {
                let risk = doc.visible_text_risk();
                let verdict = if risk.is_empty() {
                    if audit.unresolved.is_empty() {
                        "OK".to_string()
                    } else {
                        "OK（未嵌入字体只用于隐藏文字层/扫描页）".to_string()
                    }
                } else {
                    dirty.push(format!("{name}: {}", risk.join(", ")));
                    format!("[!] 缺字风险: {}", risk.join(", "))
                };
                println!(
                    "{name}\n  {:.1} MB  页数 {}  outline {} 字体 {}（未嵌入非标准 {}）  {verdict}",
                    audit.size_mb,
                    audit.pages,
                    audit.outline_total,
                    audit.fonts.len(),
                    audit.unresolved.len()
                );
                for font in &audit.unresolved {
                    println!("{}", font_line(font));
                }
                if !fonts_only_mode {
                    let out_dir = spike_dir_for(path, &per_file);
                    println!("  render -> {}", out_dir.display());
                    render_pages(&doc, &per_file, &out_dir);
                }
            }
            Err(err) => println!("{name}\n  [!] 打不开: {err}"),
        }
    }

    println!("\n=== 汇总 ===");
    println!("共 {} 本；依赖系统字体（hayro 会缺字）的 {} 本", files.len(), dirty.len());
    for line in &dirty {
        println!("  {line}");
    }
}

fn main() {
    let args = parse_args();
    if args.path.is_dir() {
        run_directory(&args.path, &args);
    } else {
        run_single(&args.path, &args);
    }
}
