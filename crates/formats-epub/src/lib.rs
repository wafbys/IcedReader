//! EPUB adapter. rbook types stay inside this crate.

mod html;

use std::path::Path;

use iced_reader_core::{
    extension_is, Book, BookOpener, CoreError, Metadata, Resource, SpineItem, TocNode, EPUB_FORMAT,
};
use rbook::ebook::manifest::ManifestEntry;
use rbook::epub::rewrite::{EpubRewriteOptions, PathRewrite};
use rbook::epub::toc::EpubTocEntry;
use rbook::epub::Epub;

use html::{href_file_key, rewrite_html_paths, slice_chapter, split_href};

pub struct EpubOpener;

impl BookOpener for EpubOpener {
    fn format_id(&self) -> &'static str {
        EPUB_FORMAT
    }

    fn can_open(&self, path: &Path) -> bool {
        extension_is(path, "epub")
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Book>, CoreError> {
        let epub = Epub::open(path).map_err(|e| CoreError::msg(e.to_string()))?;
        Ok(Box::new(EpubBook { inner: epub }))
    }
}

pub struct EpubBook {
    inner: Epub,
}

impl EpubBook {
    fn href_of_resource(resource: &rbook::ebook::resource::Resource<'_>) -> String {
        resource.key().value().unwrap_or_default().to_string()
    }

    fn href_of_entry<'a>(entry: &impl ManifestEntry<'a>) -> String {
        Self::href_of_resource(&entry.resource())
    }

    fn map_toc(entry: EpubTocEntry<'_>) -> TocNode {
        TocNode {
            label: entry.label().to_string(),
            href: entry.href().map(|h| h.as_str().to_string()),
            children: entry.iter().map(Self::map_toc).collect(),
        }
    }

    fn opf_spine(&self) -> Vec<SpineItem> {
        self.inner
            .spine()
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                let manifest = entry.manifest_entry()?;
                Some(SpineItem {
                    id: format!("spine-{i}"),
                    href: Self::href_of_entry(&manifest),
                    media_type: manifest.kind().as_str().to_string(),
                    title: None,
                })
            })
            .collect()
    }

    fn toc_spine(&self) -> Vec<SpineItem> {
        let Some(root) = self.inner.toc().contents() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        self.collect_toc_spine(root, &mut out);
        out
    }

    fn collect_toc_spine(&self, entry: EpubTocEntry<'_>, out: &mut Vec<SpineItem>) {
        if let Some(href) = entry.href() {
            let href_s = href.as_str();
            if !href_s.is_empty()
                && href_s != "/"
                && out.last().map(|prev| prev.href.as_str()) != Some(href_s)
            {
                let label = entry.label().trim();
                out.push(SpineItem {
                    id: format!("toc-{}", out.len()),
                    href: href_s.to_string(),
                    media_type: self.lookup_media_type(href.path().as_str()),
                    title: if label.is_empty() {
                        None
                    } else {
                        Some(label.to_string())
                    },
                });
            }
        }
        for child in entry.iter() {
            self.collect_toc_spine(child, out);
        }
    }

    fn with_toc_titles(mut spine: Vec<SpineItem>, toc: &[SpineItem]) -> Vec<SpineItem> {
        for item in &mut spine {
            if item.title.is_some() {
                continue;
            }
            let file = href_file_key(&item.href);
            if let Some(title) = toc
                .iter()
                .find(|t| href_file_key(&t.href) == file)
                .and_then(|t| t.title.clone())
            {
                item.title = Some(title);
            }
        }
        spine
    }

    fn next_fragment_after(&self, href: &str) -> Option<String> {
        let items = self.spine();
        let file = href_file_key(href);
        let mut seen = false;
        for item in items {
            if seen {
                if href_file_key(&item.href) == file {
                    return split_href(&item.href).1.map(str::to_string);
                }
                return None;
            }
            if hrefs_match(&item.href, href) {
                seen = true;
            }
        }
        None
    }

    fn read_chapter_document(&self, file: &str, resource_base: &str) -> Result<String, CoreError> {
        let rewrite =
            EpubRewriteOptions::new().rewrite_paths(PathRewrite::prefix(resource_base.to_string()));
        match self.inner.read_resource_str_with(file, &rewrite) {
            Ok(html) => Ok(html),
            Err(rewrite_err) => {
                let raw = self.inner.read_resource_str(file).map_err(|e| {
                    CoreError::ChapterNotFound(format!("{file}: {rewrite_err}; {e}"))
                })?;
                Ok(rewrite_html_paths(&raw, resource_base, file))
            }
        }
    }

    fn lookup_media_type(&self, href: &str) -> String {
        let needle = normalize_href(href);
        for entry in self.inner.manifest().iter() {
            if normalize_href(&Self::href_of_entry(&entry)) == needle {
                return entry.kind().as_str().to_string();
            }
        }
        guess_media_type(href)
    }
}

impl Book for EpubBook {
    fn format_id(&self) -> &'static str {
        EPUB_FORMAT
    }

    fn metadata(&self) -> Metadata {
        let meta = self.inner.metadata();
        let title = meta
            .title()
            .map(|t| t.value().to_string())
            .unwrap_or_else(|| "Untitled".into());
        let authors = meta.creators().map(|c| c.value().to_string()).collect();
        let language = meta.language().map(|l| l.value().to_string());
        let publisher = meta.publishers().next().map(|p| p.value().to_string());
        let identifiers = meta.identifiers().map(|i| i.value().to_string()).collect();
        let description = meta.description().map(|d| d.value().to_string());
        let cover_href = self
            .inner
            .manifest()
            .cover_image()
            .map(|c| Self::href_of_entry(&c));

        Metadata {
            title,
            authors,
            language,
            publisher,
            identifiers,
            description,
            cover_href,
        }
    }

    fn toc(&self) -> Vec<TocNode> {
        match self.inner.toc().contents() {
            Some(root) => root.iter().map(Self::map_toc).collect(),
            None => Vec::new(),
        }
    }

    fn spine(&self) -> Vec<SpineItem> {
        let opf = self.opf_spine();
        let toc = self.toc_spine();
        if toc.len() >= 2 && (toc.iter().any(|s| s.href.contains('#')) || toc.len() > opf.len()) {
            toc
        } else {
            Self::with_toc_titles(opf, &toc)
        }
    }

    fn chapter_html(&self, href: &str, resource_base: &str) -> Result<String, CoreError> {
        let (file, fragment) = split_href(href);
        if file.is_empty() {
            return Err(CoreError::ChapterNotFound(href.into()));
        }
        let html = self.read_chapter_document(file, resource_base)?;
        let until = self.next_fragment_after(href);
        Ok(slice_chapter(&html, fragment, until.as_deref()))
    }

    fn resource(&self, href: &str) -> Result<Resource, CoreError> {
        let data = self
            .inner
            .read_resource_bytes(href)
            .map_err(|e| CoreError::ResourceNotFound(format!("{href}: {e}")))?;
        Ok(Resource {
            media_type: self.lookup_media_type(href),
            href: href.to_string(),
            data,
        })
    }
}

fn hrefs_match(a: &str, b: &str) -> bool {
    let (file_a, frag_a) = split_href(a);
    let (file_b, frag_b) = split_href(b);
    file_a
        .trim_start_matches('/')
        .eq_ignore_ascii_case(file_b.trim_start_matches('/'))
        && frag_a == frag_b
}

fn normalize_href(href: &str) -> String {
    let stripped = href.split(['#', '?']).next().unwrap_or(href);
    stripped.trim_start_matches('/').to_string()
}

fn guess_media_type(href: &str) -> String {
    let ext = Path::new(href)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "xhtml" | "html" | "htm" => "application/xhtml+xml".into(),
        "css" => "text/css".into(),
        "js" => "text/javascript".into(),
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "gif" => "image/gif".into(),
        "svg" => "image/svg+xml".into(),
        "webp" => "image/webp".into(),
        "woff" => "font/woff".into(),
        "woff2" => "font/woff2".into(),
        "ttf" | "otf" => "font/ttf".into(),
        "ncx" => "application/x-dtbncx+xml".into(),
        _ => "application/octet-stream".into(),
    }
}

pub fn is_document(media_type: &str, href: &str) -> bool {
    let mt = media_type.to_ascii_lowercase();
    mt.contains("html")
        || mt.contains("xml")
            && href.rsplit('.').next().is_some_and(|e| {
                matches!(e.to_ascii_lowercase().as_str(), "xhtml" | "html" | "htm")
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_reader_core::BookOpener;
    use std::fs::File;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    fn write_min_epub(path: &Path) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        zip.start_file("META-INF/container.xml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
                .as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/content.opf", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:icedreader-test</dc:identifier>
    <dc:title>测试书</dc:title>
    <dc:language>zh</dc:language>
    <dc:creator>IcedReader</dc:creator>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
    <itemref idref="c2"/>
  </spine>
</package>"#
                .as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/nav.xhtml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>nav</title></head>
<body>
<nav epub:type="toc">
  <ol>
    <li><a href="ch1.xhtml">第一章</a></li>
    <li><a href="ch2.xhtml">第二章</a></li>
  </ol>
</nav>
</body>
</html>"#
                .as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/ch1.xhtml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>一</title><link rel="stylesheet" href="style.css"/></head>
<body><h1>第一章</h1><p>你好，世界。</p></body>
</html>"#
                .as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/ch2.xhtml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>二</title><link rel="stylesheet" href="style.css"/></head>
<body><h1>第二章</h1><p>下一章也能打开。</p></body>
</html>"#
                .as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/style.css", deflated).unwrap();
        zip.write_all(
            b"h1 { color: #8a3b1d; font-family: sans-serif; }\nbody { font-family: serif; }",
        )
        .unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn opens_minimal_epub() {
        let dir = std::env::temp_dir().join("icedreader-epub-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("min.epub");
        write_min_epub(&path);

        let opener = EpubOpener;
        assert!(opener.can_open(&path));
        let book = opener.open(&path).expect("open epub");
        let meta = book.metadata();
        assert_eq!(meta.title, "测试书");
        assert_eq!(meta.authors, vec!["IcedReader"]);
        assert_eq!(meta.language.as_deref(), Some("zh"));

        let spine = book.spine();
        assert_eq!(spine.len(), 2);

        let html = book
            .chapter_html(&spine[0].href, "http://icedreader.localhost/book/test/")
            .expect("chapter");
        assert!(html.contains("你好，世界"), "{html}");
        assert!(
            html.contains("http://icedreader.localhost/book/test/"),
            "resource URLs should be rewritten: {html}"
        );
        assert!(
            !html.contains("data-icedreader-fonts"),
            "format layer must not inject reader font CSS: {html}"
        );

        let fonts = iced_reader_core::collect_publisher_fonts(
            &html,
            "http://icedreader.localhost/book/test/",
            &spine[0].href,
            |href| {
                book.resource(href)
                    .ok()
                    .and_then(|r| String::from_utf8(r.data).ok())
            },
        );
        assert!(
            fonts
                .declarations
                .iter()
                .any(|d| d.selector == "body" && d.value == "serif"),
            "{:?}",
            fonts.declarations
        );
        assert!(
            fonts
                .declarations
                .iter()
                .any(|d| d.selector == "h1" && d.value == "sans-serif"),
            "{:?}",
            fonts.declarations
        );

        let toc = book.toc();
        assert!(!toc.is_empty(), "expected toc entries, got {toc:?}");
        assert_eq!(
            toc[0].href.as_deref().map(href_file_key),
            Some("epub/ch1.xhtml".into())
        );
        assert_eq!(spine[0].title.as_deref(), Some("第一章"));
    }

    fn write_fragment_epub(path: &Path) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("EPUB/content.opf", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:icedreader-frag</dc:identifier>
    <dc:title>碎片书</dc:title>
    <dc:language>zh</dc:language>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="body" href="body.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="body"/>
  </spine>
</package>"#
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("EPUB/nav.xhtml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>nav</title></head>
<body>
<nav epub:type="toc">
  <ol>
    <li><a href="body.xhtml#a">英租界</a></li>
    <li><a href="body.xhtml#b">法租界</a></li>
  </ol>
</nav>
</body>
</html>"#
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("EPUB/body.xhtml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>body</title></head>
<body>
<div class="wrap">
<h1 id="a">英租界</h1><p>维多利亚公园</p>
<h1 id="b">法租界</h1><p>克雷孟梭广场</p>
</div>
</body>
</html>"#
                .as_bytes(),
        )
        .unwrap();
        zip.finish().unwrap();
    }

    fn write_broken_html_epub(path: &Path) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OPS/fb.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("OPS/fb.opf", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:icedreader-html</dc:identifier>
    <dc:title>破书</dc:title>
    <dc:language>zh</dc:language>
    <dc:creator>海诚</dc:creator>
  </metadata>
  <manifest>
    <item id="ncx" href="fb.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="c1" href="chapter3.html" media-type="application/xhtml+xml"/>
    <item id="css" href="css/main.css" media-type="text/css"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="c1"/>
  </spine>
</package>"#
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("OPS/fb.ncx", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content="urn:uuid:icedreader-html"/>
    <meta name="dtb:depth" content="1"/>
    <meta name="dtb:totalPageCount" content="0"/>
    <meta name="dtb:maxPageNumber" content="0"/>
  </head>
  <docTitle><text>破书</text></docTitle>
  <navMap>
    <navPoint id="c1" playOrder="1">
      <navLabel><text>第一回</text></navLabel>
      <content src="chapter3.html"/>
    </navPoint>
  </navMap>
</ncx>"#
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("OPS/chapter3.html", deflated).unwrap();
        zip.write_all(
            r#"<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>第一回</title><link rel="stylesheet" href="css/main.css"/></head>
<body>
<h3>第一回 金蝉破戒</h3>
<p><img src="pic.png" width="63"></p>
</body>
</html>"#
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("OPS/css/main.css", deflated).unwrap();
        zip.write_all(b"body { font-family: serif; }").unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn toc_fragments_become_chapters() {
        let dir = std::env::temp_dir().join("icedreader-epub-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frag.epub");
        write_fragment_epub(&path);

        let book = EpubOpener.open(&path).expect("open");
        let spine = book.spine();
        assert_eq!(spine.len(), 2, "{spine:?}");
        assert!(spine[0].href.contains("#a"), "{:?}", spine[0].href);
        assert!(spine[1].href.contains("#b"), "{:?}", spine[1].href);
        assert_eq!(spine[0].title.as_deref(), Some("英租界"));
        assert_eq!(spine[1].title.as_deref(), Some("法租界"));

        let a = book
            .chapter_html(&spine[0].href, "http://icedreader.localhost/book/t/")
            .expect("a");
        assert!(a.contains("维多利亚公园"), "{a}");
        assert!(!a.contains("克雷孟梭广场"), "{a}");
        assert!(!a.contains("法租界"), "{a}");

        let b = book
            .chapter_html(&spine[1].href, "http://icedreader.localhost/book/t/")
            .expect("b");
        assert!(b.contains("克雷孟梭广场"), "{b}");
        assert!(!b.contains("维多利亚公园"), "{b}");
        assert!(!b.contains("英租界"), "{b}");

        let toc = book.toc();
        assert!(
            toc.iter()
                .any(|n| n.href.as_deref().is_some_and(|h| h.contains("#a"))),
            "{toc:?}"
        );
    }

    #[test]
    fn ill_formed_html_still_rewrites() {
        let dir = std::env::temp_dir().join("icedreader-epub-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.epub");
        write_broken_html_epub(&path);

        let book = EpubOpener.open(&path).expect("open");
        let spine = book.spine();
        assert_eq!(spine.len(), 1);
        let html = book
            .chapter_html(&spine[0].href, "http://icedreader.localhost/book/t/")
            .expect("chapter");
        assert!(html.contains("第一回 金蝉破戒"), "{html}");
        assert!(
            html.contains("http://icedreader.localhost/book/t/OPS/pic.png"),
            "{html}"
        );
        assert!(
            html.contains("http://icedreader.localhost/book/t/OPS/css/main.css"),
            "{html}"
        );
        assert_eq!(spine[0].title.as_deref(), Some("第一回"));
    }

    #[test]
    fn user_epubs_if_present() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let tj = root.join("天津往事.epub");
        if tj.exists() {
            let book = EpubOpener.open(&tj).expect("天津往事");
            let spine = book.spine();
            assert!(
                spine.len() > 20,
                "expected TOC chapters, got {} {:?}",
                spine.len(),
                spine
                    .iter()
                    .map(|s| (s.title.clone(), s.href.clone()))
                    .collect::<Vec<_>>()
            );
            assert!(
                spine.iter().any(|s| s.href.contains('#')),
                "expected fragment hrefs: {:?}",
                spine.iter().map(|s| &s.href).take(5).collect::<Vec<_>>()
            );
            let idx = spine
                .iter()
                .position(|s| s.title.as_deref() == Some("英租界：回到维多利亚时代"))
                .expect("英租界 chapter");
            let html = book
                .chapter_html(&spine[idx].href, "http://icedreader.localhost/book/t/")
                .expect("英租界 html");
            assert!(html.contains("维多利亚"), "{html}");
            assert!(
                !html.contains("英租界推广界"),
                "should not include the next TOC section: {}",
                &html[html.len().saturating_sub(200)..]
            );
            assert!(
                html.len() < 80_000,
                "chapter still too large: {}",
                html.len()
            );
        }

        let xy = root.join("新西游记++共两册.epub");
        if xy.exists() {
            let book = EpubOpener.open(&xy).expect("新西游记");
            let spine = book.spine();
            assert!(spine.len() >= 70, "spine {}", spine.len());
            book.chapter_html(&spine[1].href, "http://icedreader.localhost/book/t/")
                .expect("作者简介");
            let html = book
                .chapter_html(&spine[2].href, "http://icedreader.localhost/book/t/")
                .expect("第一回");
            assert!(html.contains("第一回"), "{html}");
            assert!(
                html.contains("http://icedreader.localhost/book/t/OPS/"),
                "paths rewritten: {html}"
            );
        }
    }
}
