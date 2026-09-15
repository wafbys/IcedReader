//! Probe: **could a text PDF be served as vector SVG instead of a raster?**
//!
//! The reader rasterises every page today. This probe answers what the vector
//! alternative would actually give us, per page:
//!
//! * does the SVG keep real `<text>` (⇒ selectable/searchable in the webview)
//!   or does it convert glyphs to `<path>` outlines (⇒ crisp at any zoom, but
//!   no text)?
//! * does it inline fonts (`@font-face` + base64) and how big does the file get
//!   compared to the WebP the reader serves now?
//!
//! `hayro-svg` is a **dev-dependency**: this is a measurement tool, not part of
//! the shipped reader.
//!
//! ```text
//! cargo run --release -p iced-reader-pdf --example svg_probe -- <file.pdf> [page]
//! ```

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro_svg::{convert, RenderCache, SvgRenderSettings};

fn count(hay: &str, needle: &str) -> usize {
    hay.matches(needle).count()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: svg_probe <file.pdf> [page]");
        std::process::exit(2);
    };
    let page_no: usize = args.get(1).and_then(|p| p.parse().ok()).unwrap_or(1);

    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("[!] 读不了 {path}: {err}");
            std::process::exit(1);
        }
    };
    let pdf = Pdf::new(bytes).expect("open pdf");
    let pages = pdf.pages();
    let Some(page) = pages.iter().nth(page_no - 1) else {
        eprintln!("[!] 第 {page_no} 页不存在");
        std::process::exit(1);
    };

    let cache = RenderCache::new();
    let started = std::time::Instant::now();
    let svg = convert(
        page,
        &cache,
        &InterpreterSettings::default(),
        &SvgRenderSettings::default(),
    );
    let ms = started.elapsed().as_secs_f64() * 1000.0;

    let name = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "probe".into());
    println!("{name} p{page_no}: SVG {:.1} KB in {ms:.1} ms", svg.len() as f64 / 1024.0);
    println!(
        "  <text={} <tspan={} <path={} <image={} @font-face={} base64-payload={}",
        count(&svg, "<text"),
        count(&svg, "<tspan"),
        count(&svg, "<path"),
        count(&svg, "<image"),
        count(&svg, "@font-face"),
        count(&svg, "base64,")
    );
    println!(
        "  first 200 chars: {}",
        svg.chars().take(200).collect::<String>().replace('\n', " ")
    );

    let out = std::env::temp_dir().join(format!("icedreader-svg-probe-{name}-p{page_no}.svg"));
    if let Err(err) = std::fs::write(&out, &svg) {
        eprintln!("[!] 写不了 {}: {err}", out.display());
    } else {
        println!("  wrote {}", out.display());
    }
}
