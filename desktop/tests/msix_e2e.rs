#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../installer/msix.ps1");
const MANIFEST: &str = include_str!("../../installer/msix/AppxManifest.xml");
const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../installer/msix/Assets");
const PACKAGE_NAME: &str = "Octo.msix";
const APP_EXE: &str = "Octo.exe";

struct Packed {
    dir: PathBuf,
}

impl Packed {
    fn layout(&self) -> PathBuf {
        self.dir.join("layout")
    }

    fn package(&self) -> PathBuf {
        self.dir.join(PACKAGE_NAME)
    }

    fn manifest(&self) -> String {
        std::fs::read_to_string(self.layout().join("AppxManifest.xml")).unwrap()
    }
}

impl Drop for Packed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn pack(name: &str, extra: &[&str]) -> Result<Packed, String> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "octo-msix-{name}-{}-{}",
        std::process::id(),
        orkai::util::now_ms()
    ));
    let packed = Packed { dir };
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", SCRIPT, "-Binary"])
        .arg(env!("CARGO_BIN_EXE_octo"))
        .arg("-Layout")
        .arg(packed.layout())
        .arg("-Out")
        .arg(packed.package())
        .args(extra)
        .env_remove("PSModulePath")
        .output()
        .unwrap();
    if output.status.success() {
        Ok(packed)
    } else {
        Err(format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
    }
}

fn referenced_assets(manifest: &str) -> Vec<String> {
    manifest
        .split('"')
        .chain(manifest.split(['<', '>']))
        .filter(|part| part.starts_with(r"Assets\") && part.ends_with(".png"))
        .map(|part| part.trim_start_matches(r"Assets\").to_string())
        .collect()
}

fn store_assets() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(ASSETS)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_store_package_writes_to_the_real_app_data_so_agents_outside_it_share_the_octo_folder() {
    assert!(
        MANIFEST.contains("<desktop6:FileSystemWriteVirtualization>disabled</desktop6:FileSystemWriteVirtualization>")
    );
    assert!(MANIFEST.contains("<desktop6:RegistryWriteVirtualization>disabled</desktop6:RegistryWriteVirtualization>"));
    assert!(MANIFEST.contains(r#"<rescap:Capability Name="unvirtualizedResources" />"#));
    assert!(MANIFEST.contains(r#"<rescap:Capability Name="runFullTrust" />"#));
    assert!(MANIFEST.contains(&format!(r#"Executable="{APP_EXE}" EntryPoint="Windows.FullTrustApplication""#)));
}

#[test]
fn the_store_installs_the_visual_c_runtime_the_app_links_against() {
    assert!(
        MANIFEST.contains(r#"<PackageDependency Name="Microsoft.VCLibs.140.00.UWPDesktop" MinVersion="14.0.30035.0""#)
    );
}

#[test]
fn every_logo_the_store_package_names_is_in_its_assets() {
    let assets = store_assets();
    let referenced = referenced_assets(MANIFEST);
    assert!(!referenced.is_empty());
    for name in referenced {
        assert!(assets.contains(&name), "{name} is missing from {ASSETS}");
    }
    for size in [16, 24, 32, 48, 256] {
        for variant in [
            format!("Square44x44Logo.targetsize-{size}.png"),
            format!("Square44x44Logo.targetsize-{size}_altform-unplated.png"),
        ] {
            assert!(assets.contains(&variant), "the taskbar icon {variant} is missing from {ASSETS}");
        }
    }
}

#[test]
#[ignore = "needs the Windows SDK's makeappx and makepri; CI runs it with --ignored"]
fn packs_the_app_under_the_store_identity_with_the_version_of_the_app() {
    let packed = pack(
        "identity",
        &[
            "-Identity",
            "12345Test.Octo",
            "-Publisher",
            "CN=00000000-1111-2222-3333-444444444444",
            "-PublisherDisplayName",
            "Test Publisher",
            "-DisplayName",
            "Octo Test",
        ],
    )
    .unwrap_or_else(|log| panic!("msix.ps1 failed:\n{log}"));
    assert!(packed.package().is_file());
    let manifest = packed.manifest();
    let version = format!("{}.0", env!("CARGO_PKG_VERSION"));
    assert!(
        manifest.contains(&format!(
            r#"<Identity Name="12345Test.Octo" Publisher="CN=00000000-1111-2222-3333-444444444444" Version="{version}" ProcessorArchitecture="x64" />"#
        )),
        "{manifest}"
    );
    assert!(manifest.contains("<DisplayName>Octo Test</DisplayName>"), "{manifest}");
    assert!(manifest.contains("<PublisherDisplayName>Test Publisher</PublisherDisplayName>"), "{manifest}");
    assert!(manifest.contains(r#"DisplayName="Octo Test""#), "{manifest}");
    assert!(!manifest.contains('{'), "every placeholder is filled:\n{manifest}");
    assert!(packed.layout().join("resources.pri").is_file());
    assert_eq!(
        std::fs::read(packed.layout().join(APP_EXE)).unwrap(),
        std::fs::read(env!("CARGO_BIN_EXE_octo")).unwrap()
    );
    for name in referenced_assets(&manifest) {
        assert!(packed.layout().join("Assets").join(&name).is_file(), "{name} is missing from the package");
    }
}

#[test]
#[ignore = "needs the Windows SDK's makeappx and makepri; CI runs it with --ignored"]
fn names_with_xml_characters_still_make_a_valid_manifest() {
    let packed = pack("escape", &["-PublisherDisplayName", "Lucas & <Co>"])
        .unwrap_or_else(|log| panic!("msix.ps1 failed:\n{log}"));
    let manifest = packed.manifest();
    assert!(manifest.contains("<PublisherDisplayName>Lucas &amp; &lt;Co&gt;</PublisherDisplayName>"), "{manifest}");
    assert!(packed.package().is_file());
}

#[test]
#[ignore = "needs the Windows SDK's makeappx and makepri; CI runs it with --ignored"]
fn a_binary_without_a_version_packs_nothing() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("octo-msix-unversioned-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let binary = dir.join("plain.exe");
    std::fs::copy(Path::new(r"C:\Windows\System32\where.exe"), &binary).unwrap();
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", SCRIPT, "-Binary"])
        .arg(&binary)
        .arg("-Layout")
        .arg(dir.join("layout"))
        .arg("-Out")
        .arg(dir.join(PACKAGE_NAME))
        .env_remove("PSModulePath")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success());
    assert!(log.contains("has no ProductVersion"), "{log}");
    assert!(!dir.join(PACKAGE_NAME).exists());
    std::fs::remove_dir_all(dir).unwrap();
}
