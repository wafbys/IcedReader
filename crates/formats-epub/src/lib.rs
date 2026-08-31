//! EPUB adapter. rbook types stay inside this crate.

use std::path::Path;

use iced_reader_core::{
    extension_is, Book, BookOpener, CoreError, Metadata, Resource, SpineItem, TocNode, EPUB_FORMAT,
};
use rbook::ebook::manifest::ManifestEntry;
use rbook::ebook::toc::TocEntry;
use rbook::epub::rewrite::{EpubRewriteOptions, PathRewrite};
use rbook::epub::Epub;

const READER_CSS: &str = r#"
html { font-size: 18px; }
body {
  margin: 0 auto;
  padding: 2rem 1.75rem 4rem;
  max-width: 42rem;
  line-height: 1.75;
  color: #1f1c18;
  background: #f6f1e8;
  font-family: "Source Han Serif SC", "Noto Serif CJK SC", "Songti SC", "SimSun",
    "Georgia", "Times New Roman", serif;
}
img, svg, video, picture { max-width: 100%; height: auto; }
a { color: #8a3b1d; }
"#;

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

    fn map_toc<'a>(entry: impl TocEntry<'a>) -> TocNode {
        let href = entry
            .resource()
            .map(|r| Self::href_of_resource(&r))
            .or_else(|| entry.manifest_entry().map(|m| Self::href_of_entry(&m)));
        TocNode {
            label: entry.label().to_string(),
            href,
            children: entry.iter().map(Self::map_toc).collect(),
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
                })
            })
            .collect()
    }

    fn chapter_html(&self, href: &str, resource_base: &str) -> Result<String, CoreError> {
        let rewrite = EpubRewriteOptions::new()
            .rewrite_paths(PathRewrite::prefix(resource_base.to_string()))
            .inject_css(READER_CSS);
        self.inner
            .read_resource_str_with(href, &rewrite)
            .map_err(|e| CoreError::ChapterNotFound(format!("{href}: {e}")))
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
        || mt.contains("xml") && href.rsplit('.').next().is_some_and(|e| {
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
</container>"#.as_bytes(),
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
</package>"#.as_bytes(),
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
</html>"#.as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/ch1.xhtml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>一</title><link rel="stylesheet" href="style.css"/></head>
<body><h1>第一章</h1><p>你好，世界。</p></body>
</html>"#.as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/ch2.xhtml", deflated).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>二</title><link rel="stylesheet" href="style.css"/></head>
<body><h1>第二章</h1><p>下一章也能打开。</p></body>
</html>"#.as_bytes(),
        )
        .unwrap();

        zip.start_file("EPUB/style.css", deflated).unwrap();
        zip.write_all(b"h1 { color: #8a3b1d; }").unwrap();
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

        let toc = book.toc();
        assert!(!toc.is_empty(), "expected toc entries, got {toc:?}");
    }
}
