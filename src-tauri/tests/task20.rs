//! Task 20 tests: semver ordering and the channel rule, release parsing (synthetic fixture
//! `update_feed.json`), the host allow-list and the test feed, SHA256SUMS parsing, file
//! verification, the throttle, "Skip this version", the installed/portable/dev decision, the
//! download folder, the installer command line, and the single place that opens connections.

use std::path::{Path, PathBuf};

use cargo_line_lib::layout::AppMode;
use cargo_line_lib::settings::AppSettings;
use cargo_line_lib::update::feed::{
    is_update, marker_visible, parse_releases, plain_notes, select_offer, setup_asset_name,
    tag_version, ApiRelease, FeedError, MAX_FEED_BYTES, MAX_NOTES_CHARS,
};
use cargo_line_lib::update::install::{
    encode_forwarded, in_app_install_allowed, installer_arguments, parse_forwarded_env, updates_dir,
};
use cargo_line_lib::update::net::{check_url, test_feed, NetError, API_URL};
use cargo_line_lib::update::verify::{expected_hash, sha256_file, verify_file, VerifyError};
use cargo_line_lib::update::{
    after_update_notice, auto_check_due, clear_updates_dir, download_to_file, release_page_allowed,
    AUTO_CHECK_INTERVAL_MS,
};
use semver::Version;

const FIXTURE: &str = include_str!("update_feed.json");

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn fixture() -> Vec<ApiRelease> {
    parse_releases(FIXTURE.as_bytes()).unwrap()
}

/* ------------------------------------------------------------ versions and channel */

#[test]
fn semver_orders_pre_releases_numerically() {
    assert!(v("1.0.0-beta.2") < v("1.0.0-beta.10"));
    assert!(v("1.0.0-beta.10") < v("1.0.0"));
    assert!(v("1.0.0-beta.1") < v("1.0.0-beta.2"));
}

#[test]
fn the_channel_rule() {
    // beta → newer beta, beta → stable
    assert!(is_update(&v("1.0.0-beta.2"), &v("1.0.0-beta.3")));
    assert!(is_update(&v("1.0.0-beta.2"), &v("1.0.0")));
    assert!(is_update(&v("1.0.0-beta.2"), &v("1.1.0-beta.1")));
    // stable ignores betas, even higher ones
    assert!(!is_update(&v("1.0.0"), &v("1.1.0-beta.1")));
    assert!(is_update(&v("1.0.0"), &v("1.0.1")));
    // an equal or lower version is never an update
    assert!(!is_update(&v("1.0.0-beta.2"), &v("1.0.0-beta.2")));
    assert!(!is_update(&v("1.0.0-beta.2"), &v("1.0.0-beta.1")));
    assert!(!is_update(&v("1.0.0"), &v("1.0.0")));
}

#[test]
fn tags_must_be_v_plus_semver() {
    assert_eq!(tag_version("v1.0.0-beta.3"), Some(v("1.0.0-beta.3")));
    for bad in ["1.0.0", "release-3", "v1.0", "vv1.0.0", "v1.0.0.0", ""] {
        assert_eq!(tag_version(bad), None, "{bad}");
    }
}

/* ------------------------------------------------------------ release parsing */

#[test]
fn the_highest_newer_release_is_offered_with_its_assets() {
    let offer = select_offer(&v("1.0.0-beta.2"), &fixture()).unwrap();
    assert_eq!(
        offer.version, "1.0.0-beta.10",
        "beta.10 > beta.9; the v2.0.0 draft is ignored"
    );
    assert_eq!(offer.tag, "v1.0.0-beta.10");
    assert_eq!(offer.published_at.as_deref(), Some("2027-01-10T12:00:00Z"));
    let setup = offer.setup.as_ref().unwrap();
    assert_eq!(setup.name, "Cargo-Line_1.0.0-beta.10_x64-setup.exe");
    assert_eq!(setup.size, 7_134_914);
    assert!(setup
        .url
        .ends_with("/Cargo-Line_1.0.0-beta.10_x64-setup.exe"));
    assert!(offer.sums.is_some());
    assert!(offer.installable());
    // Plain text: CRLF normalised, the <script> kept as text.
    assert!(offer.notes.contains("<script>alert(1)</script> stays text"));
    assert!(!offer.notes.contains('\r'));
    assert!(!offer.notes_truncated);
}

#[test]
fn drafts_and_bad_tags_are_never_offered() {
    // From 1.0.0-beta.10 nothing is newer except the draft v2.0.0 and the bad tag.
    assert_eq!(select_offer(&v("1.0.0-beta.10"), &fixture()), None);
    // A stable 0.9.0 build: only stable releases count, and v2.0.0 is a draft.
    assert_eq!(select_offer(&v("0.9.0"), &fixture()), None);
    assert_eq!(
        select_offer(&v("0.8.0"), &fixture()).unwrap().version,
        "0.9.0"
    );
}

#[test]
fn a_missing_asset_gives_a_notice_only() {
    let mut releases = fixture();
    releases.retain(|r| r.tag_name == "v1.0.0-beta.9");
    let offer = select_offer(&v("1.0.0-beta.2"), &releases).unwrap();
    assert!(offer.setup.is_some());
    assert!(offer.sums.is_none(), "beta.9 has no SHA256SUMS.txt");
    assert!(!offer.installable());
    // Assets are matched by exact name only.
    let mut renamed = fixture();
    renamed.retain(|r| r.tag_name == "v1.0.0-beta.10");
    renamed[0].assets[0].name = "Cargo Line_1.0.0-beta.10_x64-setup.exe".into();
    let offer = select_offer(&v("1.0.0-beta.2"), &renamed).unwrap();
    assert!(offer.setup.is_none());
    assert!(!offer.installable());
    assert_eq!(
        setup_asset_name(&v("1.0.0-beta.3")),
        "Cargo-Line_1.0.0-beta.3_x64-setup.exe"
    );
}

#[test]
fn an_oversized_or_broken_body_is_refused() {
    let big = vec![b' '; MAX_FEED_BYTES + 1];
    assert_eq!(parse_releases(&big).unwrap_err(), FeedError::TooLarge);
    assert!(matches!(
        parse_releases(b"{\"message\":\"x\"}"),
        Err(FeedError::Invalid(_))
    ));
    assert!(parse_releases(b"[]").unwrap().is_empty());
}

#[test]
fn long_notes_are_capped_and_control_characters_removed() {
    let long = "x".repeat(MAX_NOTES_CHARS + 50);
    let (text, cut) = plain_notes(&long);
    assert!(cut);
    assert_eq!(text.chars().count(), MAX_NOTES_CHARS + 1, "capped, plus …");
    let (text, cut) = plain_notes("a\u{0007}b\r\nc\td");
    assert!(!cut);
    assert_eq!(text, "ab\nc\td");
}

/* ------------------------------------------------------------ hosts */

#[test]
fn the_host_allow_list() {
    for ok in [
        API_URL,
        "https://github.com/MathisVkg/cargoline/releases/download/v1.0.0-beta.3/SHA256SUMS.txt",
        "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r&sig=x",
        "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2",
    ] {
        assert!(check_url(ok, None).is_ok(), "{ok}");
    }
    for bad in [
        "http://api.github.com/repos/MathisVkg/cargoline/releases",
        "https://api.github.com/repos/someone/else/releases",
        "https://github.com/MathisVkg/cargoline/archive/main.zip",
        "https://github.com/evil/cargoline/releases/download/v1/x.exe",
        "https://example.com/Cargo-Line_1.0.0_x64-setup.exe",
        "https://github.com.evil.example/MathisVkg/cargoline/releases/download/v1/x",
        "https://user:pw@api.github.com/repos/MathisVkg/cargoline/releases",
        "https://api.github.com:8443/repos/MathisVkg/cargoline/releases",
        "http://127.0.0.1:8080/releases.json",
        "file:///C:/Windows/notepad.exe",
        "not a url",
    ] {
        assert!(
            matches!(check_url(bad, None), Err(NetError::Refused(_))),
            "{bad}"
        );
    }
    let ok = check_url(API_URL, None).unwrap();
    assert!(ok.secure);
    assert_eq!(ok.host, "api.github.com");
    assert_eq!(ok.port, 443);
    assert_eq!(ok.target, "/repos/MathisVkg/cargoline/releases?per_page=20");
}

#[test]
fn a_redirect_target_is_checked_like_any_url() {
    // get() resolves the Location against the current URL and checks it again: a redirect from
    // github.com to anywhere else is refused the same way.
    let base =
        tauri::Url::parse("https://github.com/MathisVkg/cargoline/releases/download/v1/x.exe")
            .unwrap();
    for (location, allowed) in [
        ("https://release-assets.githubusercontent.com/a/b?c=d", true),
        ("https://evil.example/x.exe", false),
        ("http://release-assets.githubusercontent.com/a", false),
        ("/someone/else/releases/download/v1/x.exe", false),
    ] {
        let next = base.join(location).unwrap().to_string();
        assert_eq!(check_url(&next, None).is_ok(), allowed, "{location}");
    }
}

#[test]
fn the_test_feed_is_loopback_http_only() {
    let feed = test_feed(Some("http://127.0.0.1:8765/releases.json")).unwrap();
    assert_eq!(feed.port(), Some(8765));
    for bad in [
        "https://127.0.0.1:8765/releases.json",
        "http://localhost:8765/releases.json",
        "http://127.0.0.1/releases.json",
        "http://192.168.1.2:8765/x",
        "http://user@127.0.0.1:8765/x",
        "https://api.github.com/repos/MathisVkg/cargoline/releases",
        "",
    ] {
        assert!(test_feed(Some(bad)).is_none(), "{bad}");
    }
    assert!(test_feed(None).is_none());
    // With the feed set, that loopback address (same port) is allowed; nothing else changes.
    assert!(check_url("http://127.0.0.1:8765/assets/SHA256SUMS.txt", Some(&feed)).is_ok());
    assert!(check_url("http://127.0.0.1:9999/x", Some(&feed)).is_err());
    assert!(check_url("http://localhost:8765/x", Some(&feed)).is_err());
    assert!(check_url("https://example.com/x", Some(&feed)).is_err());
    assert!(check_url(API_URL, Some(&feed)).is_ok());
    // Without the feed, loopback is refused.
    assert!(check_url("http://127.0.0.1:8765/assets/SHA256SUMS.txt", None).is_err());
}

#[test]
fn the_release_page_pattern() {
    assert!(release_page_allowed(
        "https://github.com/MathisVkg/cargoline/releases/tag/v1.0.0-beta.3"
    ));
    assert!(!release_page_allowed(
        "https://github.com/MathisVkg/cargoline/releases/tag/latest"
    ));
    assert!(!release_page_allowed(
        "https://github.com/evil/cargoline/releases/tag/v1.0.0"
    ));
    assert!(!release_page_allowed(
        "https://evil.example/MathisVkg/cargoline/releases/tag/v1.0.0"
    ));
}

/* ------------------------------------------------------------ SHA256SUMS and verification */

const SETUP: &str = "Cargo-Line_1.0.0-beta.1_x64-setup.exe";
const ZIP: &str = "Cargo-Line_1.0.0-beta.1_x64-portable.zip";
const HASH_SETUP: &str = "c66d21d57e1f9269d1b203d59753da50d85535b61303be7b807f4ed694cfa8ef";
const HASH_ZIP: &str = "630c05dc3c73472bbd88351ce5785805277ea84a5e5e2edca109bf8d348b46d6";

#[test]
fn sha256sums_parsing() {
    // Task 19 format: "<hash>  <name>", LF.
    let task19 = format!("{HASH_SETUP}  {SETUP}\n{HASH_ZIP}  {ZIP}\n");
    assert_eq!(expected_hash(&task19, SETUP).as_deref(), Some(HASH_SETUP));
    assert_eq!(expected_hash(&task19, ZIP).as_deref(), Some(HASH_ZIP));
    // CRLF, one space, binary-mode star, upper-case hex.
    let other = format!(
        "{}  {SETUP}\r\n{} *{ZIP}\r\n",
        HASH_SETUP.to_uppercase(),
        HASH_ZIP
    );
    assert_eq!(
        expected_hash(&other, SETUP).as_deref(),
        Some(HASH_SETUP),
        "lower-cased"
    );
    assert_eq!(expected_hash(&other, ZIP).as_deref(), Some(HASH_ZIP));
    // A missing line, a similar name, a broken hash.
    assert_eq!(
        expected_hash(&task19, "Cargo-Line_1.0.0-beta.2_x64-setup.exe"),
        None
    );
    assert_eq!(
        expected_hash(&task19, "Cargo-Line_1.0.0-beta.1_x64-setup"),
        None
    );
    assert_eq!(expected_hash(&format!("abc123  {SETUP}\n"), SETUP), None);
}

#[test]
fn sha256_of_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("abc.bin");
    std::fs::write(&file, b"abc").unwrap();
    assert_eq!(
        sha256_file(&file).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let big = dir.path().join("big.bin");
    std::fs::write(&big, vec![b'a'; 1_000_000]).unwrap();
    assert_eq!(
        sha256_file(&big).unwrap(),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        "streamed over several chunks"
    );
}

#[test]
fn verification_match_mismatch_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let file = dir.path().join(SETUP);
    std::fs::write(&file, b"abc").unwrap();
    verify_file(&file, 3, abc).unwrap();
    verify_file(&file, 3, &abc.to_uppercase()).unwrap();
    assert!(file.exists(), "a good file is kept");

    assert_eq!(
        verify_file(&file, 4, abc).unwrap_err(),
        VerifyError::Size {
            actual: 3,
            expected: 4
        }
    );
    assert!(!file.exists(), "a size mismatch deletes the file");

    std::fs::write(&file, b"abd").unwrap();
    assert_eq!(verify_file(&file, 3, abc).unwrap_err(), VerifyError::Hash);
    assert!(!file.exists(), "a hash mismatch deletes the file");
}

/* ------------------------------------------------------------ throttle, skip, notice */

#[test]
fn the_automatic_check_throttle() {
    let now = 1_800_000_000_000;
    assert!(auto_check_due(true, None, now), "never checked");
    assert!(
        !auto_check_due(true, Some(now - 60_000), now),
        "checked a minute ago"
    );
    assert!(!auto_check_due(
        true,
        Some(now - AUTO_CHECK_INTERVAL_MS + 1),
        now
    ));
    assert!(
        auto_check_due(true, Some(now - AUTO_CHECK_INTERVAL_MS), now),
        "24 h later"
    );
    assert!(
        auto_check_due(true, Some(now + 3_600_000), now),
        "the clock went back"
    );
    assert!(
        !auto_check_due(false, None, now),
        "setting off: no automatic check"
    );
    assert!(!auto_check_due(false, Some(0), now));
    // A manual check never consults the throttle: `update::check` has no such condition (it is
    // only applied in `update::start`). Checked in the source so it stays that way.
    let source = include_str!("../src/update/mod.rs");
    let check_fn =
        &source[source.find("pub fn check<").unwrap()..source.find("pub fn start<").unwrap()];
    assert!(!check_fn.contains("auto_check_due"));
}

#[test]
fn skip_this_version_hides_that_version_only() {
    assert!(marker_visible("1.0.0-beta.3", None));
    assert!(!marker_visible("1.0.0-beta.3", Some("1.0.0-beta.3")));
    assert!(
        marker_visible("1.0.0-beta.4", Some("1.0.0-beta.3")),
        "a higher version shows again"
    );
}

#[test]
fn update_settings_defaults_and_validation() {
    let s = AppSettings::default();
    assert!(s.check_updates_automatically);
    assert_eq!(s.update_last_check_ms, None);
    let mut bad = AppSettings {
        update_skipped_version: Some("beta".into()),
        ..Default::default()
    };
    assert!(bad.validate().is_err());
    bad.update_skipped_version = Some("1.0.0-beta.3".into());
    bad.update_last_check_ms = Some(-1);
    assert!(bad.validate().is_err());
    bad.update_last_check_ms = Some(1);
    bad.update_installing_version = Some("1.0.0-beta.3".into());
    assert!(bad.validate().is_ok());
}

#[test]
fn the_notice_after_an_update() {
    assert_eq!(
        after_update_notice(Some("1.0.0-beta.3"), "1.0.0-beta.3"),
        Some((true, "Updated to version 1.0.0-beta.3".into()))
    );
    let (ok, text) = after_update_notice(Some("1.0.0-beta.3"), "1.0.0-beta.2").unwrap();
    assert!(!ok);
    assert!(text.contains("1.0.0-beta.2 is still installed"), "{text}");
    assert_eq!(after_update_notice(None, "1.0.0-beta.3"), None);
}

/* ------------------------------------------------------------ installed or not */

#[test]
fn in_app_install_needs_the_uninstaller_and_the_registry_location() {
    let dir = Path::new(r"C:\Users\someone\AppData\Local\Cargo Line");
    let quoted = r#""C:\Users\someone\AppData\Local\Cargo Line""#;
    assert!(in_app_install_allowed(
        AppMode::Installed,
        dir,
        true,
        Some(quoted)
    ));
    assert!(in_app_install_allowed(
        AppMode::Installed,
        dir,
        true,
        Some(r"c:\users\someone\appdata\local\cargo line\")
    ));
    // A portable copy whose portable.txt was deleted: installed mode, but no uninstaller.
    assert!(!in_app_install_allowed(
        AppMode::Installed,
        dir,
        false,
        Some(quoted)
    ));
    // An installed copy elsewhere (the registry points at another folder).
    assert!(!in_app_install_allowed(
        AppMode::Installed,
        Path::new(r"D:\Copy"),
        true,
        Some(quoted)
    ));
    assert!(!in_app_install_allowed(AppMode::Installed, dir, true, None));
    assert!(!in_app_install_allowed(
        AppMode::Installed,
        dir,
        true,
        Some("")
    ));
    assert!(!in_app_install_allowed(
        AppMode::Portable,
        dir,
        true,
        Some(quoted)
    ));
    assert!(!in_app_install_allowed(
        AppMode::Dev,
        dir,
        true,
        Some(quoted)
    ));
}

#[test]
fn the_installer_command_line() {
    let dir = Path::new(r"C:\Users\someone\AppData\Local\Cargo Line");
    assert_eq!(
        installer_arguments(dir, &[]).unwrap(),
        r"/P /UPDATE /R /D=C:\Users\someone\AppData\Local\Cargo Line"
    );
    let forwarded = vec![
        (
            "CARGO_LINE_DATA_DIR".to_string(),
            r"D:\test data".to_string(),
        ),
        (
            "CARGO_LINE_UPDATE_FEED".to_string(),
            "http://127.0.0.1:8765/releases.json".to_string(),
        ),
    ];
    let hex = encode_forwarded(&forwarded).unwrap();
    assert_eq!(
        installer_arguments(Path::new(r"D:\inst\"), &forwarded).unwrap(),
        format!(r"/P /UPDATE /R /ARGS --forward={hex} /D=D:\inst")
    );
    // One token: no space, quote or slash for NSIS's option parser to act on.
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    // A newline inside a value can't be encoded line by line: refused, never passed on half.
    let bad = vec![("CARGO_LINE_DATA_DIR".to_string(), "D:\\a\nb".to_string())];
    assert!(installer_arguments(dir, &bad).is_err());
}

#[test]
fn forwarded_overrides_round_trip_for_allowed_names_only() {
    let forwarded = vec![
        (
            "CARGO_LINE_DATA_DIR".to_string(),
            r#"D:\test "data" /R x"#.to_string(),
        ),
        (
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS".to_string(),
            "--remote-debugging-port=9222".to_string(),
        ),
        ("PATH".to_string(), r"C:\evil".to_string()),
    ];
    let token = format!("--forward={}", encode_forwarded(&forwarded).unwrap());
    // NSIS passes the /ARGS text on as typed, possibly with a leading space.
    let back = parse_forwarded_env([format!(" {token}"), "unrelated".to_string()]);
    assert_eq!(back, forwarded[..2].to_vec(), "PATH is not an allowed name");
    // The quoted form NSIS mangled in the desktop session is no longer read at all.
    assert!(parse_forwarded_env([r"--env=CARGO_LINE_DATA_DIR=D:\x".to_string()]).is_empty());
    for bad in ["--forward=zz", "--forward=abc", "--forward="] {
        assert!(parse_forwarded_env([bad.to_string()]).is_empty(), "{bad}");
    }
}

/* ------------------------------------------------------------ download folder */

#[test]
fn a_partial_download_never_counts_as_complete() {
    let root = tempfile::tempdir().unwrap();
    let dir = updates_dir(root.path());
    assert_eq!(dir, root.path().join("updates"));
    // A failed write leaves neither the partial nor the final file.
    let err = download_to_file(&dir, SETUP, |file| {
        use std::io::Write;
        file.write_all(b"half").unwrap();
        Err("connection lost".into())
    });
    assert_eq!(err.unwrap_err(), "connection lost");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    // A finished write is renamed to the final name; no partial is left.
    let path = download_to_file(&dir, SETUP, |file| {
        use std::io::Write;
        file.write_all(b"abc").map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(path, dir.join(SETUP));
    let names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(names, vec![dir.join(SETUP)]);
    // The next start removes the folder.
    std::fs::write(dir.join(format!("{SETUP}.partial")), b"left over").unwrap();
    clear_updates_dir(&dir);
    assert!(!dir.exists());
    clear_updates_dir(&dir); // nothing to do: no error
}

/* ------------------------------------------------------------ one place for the network */

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn only_update_net_opens_network_connections() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    let markers = [
        "WinHttpOpen",
        "WinHttpConnect",
        "WinHttpSendRequest",
        "WinINet",
        "InternetOpen",
        "URLDownloadToFile",
        "TcpStream",
        "UdpSocket",
        "TcpListener",
        "reqwest",
        "ureq",
        "hyper::",
        "WSAStartup",
    ];
    let net = src.join("update").join("net.rs");
    let mut found = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for m in markers {
            if text.contains(m) && file != &net {
                found.push(format!("{} uses {m}", file.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "network access outside update/net.rs: {found:?}"
    );
    let net_text = std::fs::read_to_string(&net).unwrap();
    assert!(
        net_text.contains("WinHttpConnect"),
        "the one place is update/net.rs"
    );
    // And no HTTP client crate in the manifest.
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for c in [
        "reqwest",
        "ureq",
        "hyper",
        "attohttpc",
        "minreq",
        "isahc",
        "curl",
        "tauri-plugin-http",
        "tauri-plugin-updater",
    ] {
        assert!(
            !manifest.contains(&format!("\n{c} ")) && !manifest.contains(&format!("\n{c}=")),
            "{c} in Cargo.toml"
        );
    }
}
