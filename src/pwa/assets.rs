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
        path: "/model.js",
        content_type: "text/javascript; charset=utf-8",
        cache: REVALIDATE,
        body: include_bytes!("web/model.js"),
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
        path: "/octo-kai-peek.png",
        content_type: "image/png",
        cache: IMMUTABLE,
        body: include_bytes!("../../assets/octo-kai-peek.png"),
    },
    Asset {
        path: "/kai/idle.svg",
        content_type: "image/svg+xml",
        cache: IMMUTABLE,
        body: include_bytes!("../../desktop/ui/kai/idle.svg"),
    },
    Asset {
        path: "/kai/working.svg",
        content_type: "image/svg+xml",
        cache: IMMUTABLE,
        body: include_bytes!("../../desktop/ui/kai/working.svg"),
    },
    Asset {
        path: "/kai/needs.svg",
        content_type: "image/svg+xml",
        cache: IMMUTABLE,
        body: include_bytes!("../../desktop/ui/kai/needs.svg"),
    },
    Asset {
        path: "/kai/done.svg",
        content_type: "image/svg+xml",
        cache: IMMUTABLE,
        body: include_bytes!("../../desktop/ui/kai/done.svg"),
    },
    Asset {
        path: "/kai/lost.svg",
        content_type: "image/svg+xml",
        cache: IMMUTABLE,
        body: include_bytes!("../../desktop/ui/kai/lost.svg"),
    },
    Asset {
        path: "/fonts/sans-400.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/Barlow-400.woff2"),
    },
    Asset {
        path: "/fonts/sans-600.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/Barlow-600.woff2"),
    },
    Asset {
        path: "/fonts/sans-800.woff2",
        content_type: "font/woff2",
        cache: IMMUTABLE,
        body: include_bytes!("../../site/assets/fonts/Barlow-800.woff2"),
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
    fn the_manifest_is_a_standalone_octo_app_in_the_brand_colors() {
        let manifest: serde_json::Value =
            serde_json::from_slice(lookup("/manifest.webmanifest").unwrap().body).unwrap();
        assert_eq!(manifest["display"], "standalone");
        assert_eq!(manifest["name"], "Octo");
        assert_eq!(manifest["background_color"], "#121218");
        assert_eq!(manifest["start_url"], "/");
    }

    #[test]
    fn fonts_and_the_icon_are_cached_for_good_and_the_code_is_revalidated() {
        assert_eq!(lookup("/fonts/mono-400.woff2").unwrap().cache, IMMUTABLE);
        assert_eq!(lookup("/octo-kai-peek.png").unwrap().cache, IMMUTABLE);
        assert_eq!(lookup("/kai/idle.svg").unwrap().cache, IMMUTABLE);
        assert_eq!(lookup("/app.js").unwrap().cache, REVALIDATE);
        assert_eq!(lookup("/sw.js").unwrap().cache, REVALIDATE);
    }

    #[test]
    fn the_phone_shows_the_pixel_kai_of_the_desktop_app_and_its_icon() {
        let manifest: serde_json::Value =
            serde_json::from_slice(lookup("/manifest.webmanifest").unwrap().body).unwrap();
        assert_eq!(manifest["icons"][0]["src"], "/octo-kai-peek.png");
        assert_eq!(manifest["icons"][0]["sizes"], "512x512");
        let icon = lookup("/octo-kai-peek.png").unwrap().body;
        assert_eq!(&icon[1..4], b"PNG");
        assert_eq!(u32::from_be_bytes(icon[16..20].try_into().unwrap()), 512);
        assert_eq!(u32::from_be_bytes(icon[20..24].try_into().unwrap()), 512);
        assert_eq!(icon, include_bytes!("../../desktop/icons/icon.png").as_slice(), "the same icon as the desktop app");
        assert_ne!(icon, include_bytes!("../../assets/orkai.png").as_slice(), "the orkai CLI sailor stays on the CLI");
        assert!(lookup("/icon.png").is_none(), "a new path, so phones drop the sailor they cached for good");
        assert!(lookup("/octo.png").is_none(), "a new path, so phones drop the old Kai icon they cached for good");
        assert!(lookup("/octo-eye.png").is_none(), "a new path, so phones drop the smooth eye they cached for good");
        assert!(
            lookup("/octo-eye-pixel.png").is_none(),
            "a new path, so phones drop the pixel eye they cached for good"
        );
        assert!(
            lookup("/octo-kai-corner.png").is_none(),
            "a new path, so phones drop the outlined pixel tile they cached for good"
        );
        assert!(text("/sw.js").contains("'/octo-kai-peek.png'"), "the worker installs only if every shell file exists");
        assert!(!text("/sw.js").contains("'orkai-pwa-v8'"), "a new cache, so the worker drops the old shell");
        assert!(text("/").contains(r#"<link rel="icon" href="/octo-kai-peek.png" />"#));
        assert!(text("/").contains(r#"<link rel="apple-touch-icon" href="/octo-kai-peek.png" />"#));
        for mood in ["idle", "working", "needs", "done", "lost"] {
            let strip = lookup(&format!("/kai/{mood}.svg")).unwrap();
            assert_eq!(strip.content_type, "image/svg+xml");
            assert!(strip.body.starts_with(b"<svg "), "{mood}");
            assert!(text("/style.css").contains(&format!("url(\"/kai/{mood}.svg\")")), "{mood}");
        }
        assert!(text("/").contains(r#"id="mascot" class="kai mascot""#));
        assert!(text("/app.js").contains("el.mascot.dataset.mood = moodOf(agents());"));
    }

    #[test]
    fn the_phone_speaks_the_type_of_the_desktop_app_and_caches_it_for_good() {
        for weight in ["400", "600", "800"] {
            let path = format!("/fonts/sans-{weight}.woff2");
            let font = lookup(&path).unwrap();
            assert_eq!(font.cache, IMMUTABLE, "{path}");
            assert_eq!(&font.body[..4], b"wOF2", "{path}");
            assert!(text("/style.css").contains(&format!("url(\"{path}\")")), "{path}");
            assert!(text("/sw.js").contains(&format!("'{path}'")), "{path}");
        }
        assert!(!text("/sw.js").contains("'orkai-pwa-v7'"), "a new cache, so the worker drops the old shell");
    }

    #[test]
    fn the_page_loads_the_model_before_the_app_and_keeps_inline_styles_out_of_the_csp() {
        let page = text("/");
        let model = page.find(r#"<script src="/model.js"></script>"#).expect("the page loads model.js");
        let app = page.find(r#"<script src="/app.js"></script>"#).expect("the page loads app.js");
        assert!(model < app, "app.js reads OrkaiModel when it boots");
        assert_eq!(lookup("/model.js").unwrap().cache, REVALIDATE);
        assert!(text("/sw.js").contains("'/model.js'"));
        assert!(!page.contains("style=\""), "the CSP refuses inline style attributes");
        assert!(!page.contains("<script>"), "the CSP refuses inline scripts");
    }

    #[test]
    fn the_themes_match_the_desktop_app() {
        let css = text("/style.css");
        for theme in ["gray", "light"] {
            assert!(css.contains(&format!(":root[data-theme=\"{theme}\"]")), "{theme}");
        }
        assert!(text("/app.js").contains("const THEME_KEY = 'orkai.theme';"), "the same key as the desktop theme");
        assert!(text("/model.js").contains("const THEMES = ['dark', 'gray', 'light', 'system'];"));
        for index in 0..16 {
            assert!(css.contains(&format!("--ansi-{index}:")), "the terminal palette follows the theme: {index}");
        }
    }
}
