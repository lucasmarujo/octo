pub struct Asset {
    pub path: &'static str,
    pub content_type: &'static str,
    pub cache: &'static str,
    pub body: &'static [u8],
}

const REVALIDATE: &str = "no-cache";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

pub const ASSETS: &[Asset] = &[
    Asset {
        path: "/",
        content_type: "text/html; charset=utf-8",
        cache: REVALIDATE,
        body: include_bytes!("web/index.html"),
    },
    Asset {
        path: "/app.js",
        content_type: "text/javascript; charset=utf-8",
        cache: REVALIDATE,
        body: include_bytes!("web/app.js"),
    },
    Asset {
        path: "/wire.js",
        content_type: "text/javascript; charset=utf-8",
        cache: REVALIDATE,
        body: include_bytes!("web/wire.js"),
    },
    Asset {
        path: "/style.css",
        content_type: "text/css; charset=utf-8",
        cache: REVALIDATE,
        body: include_bytes!("web/style.css"),
    },
    Asset {
        path: "/sw.js",
        content_type: "text/javascript; charset=utf-8",
        cache: REVALIDATE,
        body: include_bytes!("web/sw.js"),
    },
    Asset {
        path: "/manifest.webmanifest",
        content_type: "application/manifest+json; charset=utf-8",
        cache: REVALIDATE,
        body: include_bytes!("web/manifest.webmanifest"),
    },
    Asset {
        path: "/icon.png",
        content_type: "image/png",
        cache: IMMUTABLE,
        body: include_bytes!("../../assets/orkai.png"),
    },
    Asset {
        path: "/fonts/mono-400.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/JetBrainsMono-Regular.woff2"),
    },
    Asset {
        path: "/fonts/mono-700.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/JetBrainsMono-Bold.woff2"),
    },
    Asset {
        path: "/fonts/sym-braille.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/orkai-sym-braille.woff2"),
    },
    Asset {
        path: "/fonts/sym-arrows.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/orkai-sym-arrows.woff2"),
    },
    Asset {
        path: "/fonts/sym-tech.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/orkai-sym-tech.woff2"),
    },
];

pub fn lookup(path: &str) -> Option<&'static Asset> {
    let path = if path == "/index.html" { "/" } else { path };
    ASSETS.iter().find(|asset| asset.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(path: &str) -> String {
        String::from_utf8(lookup(path).unwrap().body.to_vec()).unwrap()
    }

    #[test]
    fn every_file_the_shell_references_is_served() {
        let shell = format!("{}{}{}", text("/"), text("/style.css"), text("/sw.js"));
        let mut referenced: Vec<&str> = Vec::new();
        let is_path = |piece: &&str| {
            piece.len() > 1
                && piece.starts_with('/')
                && piece.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
        };
        for quote in ['"', '\''] {
            referenced.extend(shell.split(quote).filter(is_path));
        }
        for piece in shell.split("url(").skip(1) {
            referenced.push(piece.split(')').next().unwrap().trim_matches(['"', '\'']));
        }
        assert!(referenced.len() > 5, "{referenced:?}");
        let live = [crate::pwa::http::SOCKET_PATH, crate::pwa::http::HEALTH_PATH];
        for path in referenced.into_iter().filter(|path| !live.contains(path)) {
            assert!(lookup(path).is_some(), "{path} is referenced but not served");
        }
    }

    #[test]
    fn every_element_the_app_looks_up_is_in_the_page() {
        let (page, app) = (text("/"), text("/app.js"));
        let ids: Vec<&str> = app.split("$('").skip(1).map(|piece| piece.split('\'').next().unwrap()).collect();
        assert!(ids.contains(&"pair-form") && ids.contains(&"pair-link"), "{ids:?}");
        for id in ids {
            assert!(page.contains(&format!("id=\"{id}\"")), "app.js looks up #{id}, which the page lacks");
        }
    }

    #[test]
    fn unknown_and_escaping_paths_are_not_served() {
        assert!(lookup("/secret").is_none());
        assert!(lookup("/../Cargo.toml").is_none());
        assert!(lookup("/fonts/../../Cargo.toml").is_none());
        assert_eq!(lookup("/index.html").unwrap().path, "/");
    }

    #[test]
    fn the_manifest_is_a_standalone_app_in_the_orkai_colors() {
        let manifest: serde_json::Value =
            serde_json::from_slice(lookup("/manifest.webmanifest").unwrap().body).unwrap();
        assert_eq!(manifest["display"], "standalone");
        assert_eq!(manifest["name"], "Orkai");
        assert_eq!(manifest["background_color"], "#121218");
        assert_eq!(manifest["start_url"], "/");
    }

    #[test]
    fn fonts_and_the_icon_are_cached_for_good_and_the_code_is_revalidated() {
        assert_eq!(lookup("/fonts/mono-400.woff2").unwrap().cache, IMMUTABLE);
        assert_eq!(lookup("/icon.png").unwrap().cache, IMMUTABLE);
        assert_eq!(lookup("/app.js").unwrap().cache, REVALIDATE);
        assert_eq!(lookup("/sw.js").unwrap().cache, REVALIDATE);
    }
}
