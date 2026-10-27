//! The Plug-in Library in a full session, against a local catalog (`file://`): install, update,
//! remove and start-up loading of WebAssembly packages, checksum and URL rules.

use std::path::{Path, PathBuf};

use effectcraft_engine::Session;
use effectcraft_engine::project::{GroupKind, LayerId, Node};
use effectcraft_engine::segment::sha256;
use serde_json::{Value, json};

use super::plugins::wat_plugin;

/// A fresh folder for one test: the plug-ins folder and the catalog's files.
fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("effectcraft-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("served")).unwrap();
    d
}

fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

fn session(dir: &Path) -> Session {
    let mut s = effectcraft_host::session();
    s.plugins_dir = Some(dir.join("Plug-ins"));
    s
}

/// A served module for `effect`; returns (url, sha256).
fn serve(dir: &Path, file: &str, effect: &str) -> (String, String) {
    let bytes = wat_plugin(effect, 1, "i32.const 0").into_bytes();
    let path = dir.join("served").join(file);
    std::fs::write(&path, &bytes).unwrap();
    (file_url(&path), sha256::hex(&bytes))
}

/// Write the catalog (`packages`) and return its URL.
fn write_catalog(dir: &Path, packages: Value) -> String {
    let path = dir.join("served").join("index.json");
    std::fs::write(&path, json!({"schema": 1, "packages": packages}).to_string()).unwrap();
    file_url(&path)
}

fn release(version: &str, (url, sha): &(String, String), effect: &str) -> Value {
    json!({"version": version, "api": 1, "url": url, "sha256": sha, "effects": [effect]})
}

fn package<'a>(list: &'a Value, id: &str) -> &'a Value {
    list["packages"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("no {id} in {list}"))
}

fn plugin_ids(s: &mut Session) -> Vec<String> {
    let l = s.execute("effect.plugins.list", json!({})).unwrap();
    l["plugins"].as_array().unwrap().iter().filter_map(|p| p["id"].as_str().map(str::to_string)).collect()
}

#[test]
fn install_update_and_remove_a_package() {
    let dir = tmp("cycle");
    let fx = "org.test.library.cycle";
    let v1 = serve(&dir, "cycle-1.wasm", fx);
    let catalog = write_catalog(&dir, json!([{"id": "org.test.cycle", "name": "Cycle", "author": "Tester", "license": "MIT", "tags": ["test"], "versions": [release("1.0.0", &v1, fx)]}]));
    let mut s = session(&dir);
    // Before a catalog: nothing listed, installing is refused.
    assert_eq!(s.execute("library.list", json!({})).unwrap()["packages"], json!([]));
    assert!(s.execute("library.install", json!({"id": "org.test.cycle", "wait": true})).unwrap_err().to_string().contains("library.refresh"));
    assert_eq!(s.execute("library.refresh", json!({"catalog": catalog, "wait": true})).unwrap()["packages"], 1);
    let l = s.execute("library.list", json!({"query": "tester"})).unwrap();
    let p = package(&l, "org.test.cycle");
    assert_eq!((p["latest"].as_str(), p["installed"].as_str(), p["effects"][0].as_str()), (Some("1.0.0"), None, Some(fx)), "{l}");
    assert_eq!(l["tags"], json!(["test"]));
    assert_eq!(s.execute("library.list", json!({"query": "no such thing"})).unwrap()["packages"], json!([]));

    let r = s.execute("library.install", json!({"id": "org.test.cycle", "wait": true})).unwrap();
    assert_eq!((r["installed"].as_bool(), r["restartNeeded"].as_bool()), (Some(true), Some(false)), "{r}");
    assert!(plugin_ids(&mut s).iter().any(|i| i == fx), "loaded right away");
    let lib = dir.join("Plug-ins").join("Library");
    assert!(lib.join("org.test.cycle").join("1.0.0").join("org.test.cycle.wasm").is_file());
    let installed: Value = serde_json::from_slice(&std::fs::read(lib.join("installed.json")).unwrap()).unwrap();
    assert_eq!(installed["packages"]["org.test.cycle"]["version"], "1.0.0");
    // The effect applies like any other.
    s.execute("comp.new", json!({"name": "L", "width": 8, "height": 8, "duration": 1})).unwrap();
    let layer = s.execute("layer.newSolid", json!({"color": [1, 0, 0], "width": 8, "height": 8})).unwrap()["layer"].clone();
    s.execute("effect.apply", json!({"layer": layer, "effect": fx})).unwrap();
    assert_eq!(s.execute("library.missing", json!({})).unwrap()["effects"], json!([]));

    // Version 2: an update; the loaded version stays until the next start.
    let v2 = serve(&dir, "cycle-2.wasm", fx);
    let catalog = write_catalog(&dir, json!([{"id": "org.test.cycle", "name": "Cycle", "versions": [release("1.0.0", &v1, fx), release("1.10.0", &v2, fx)]}]));
    s.execute("library.refresh", json!({"catalog": catalog, "wait": true})).unwrap();
    assert_eq!(package(&s.execute("library.list", json!({})).unwrap(), "org.test.cycle")["updateAvailable"], true);
    let r = s.execute("library.update", json!({"wait": true})).unwrap();
    assert_eq!(r["updating"], json!(["org.test.cycle"]), "{r}");
    let p = s.execute("library.list", json!({"installed": true})).unwrap();
    let p = package(&p, "org.test.cycle");
    assert_eq!((p["installed"].as_str(), p["updateAvailable"].as_bool(), p["restartNeeded"].as_bool()), (Some("1.10.0"), Some(false), Some(true)), "{p}");
    assert!(!lib.join("org.test.cycle").join("1.0.0").exists(), "the old version is removed");

    let r = s.execute("library.uninstall", json!({"id": "org.test.cycle"})).unwrap();
    assert_eq!((r["removed"].as_bool(), r["restartNeeded"].as_bool()), (Some(true), Some(true)));
    assert!(!lib.join("org.test.cycle").exists());
    assert_eq!(s.execute("library.uninstall", json!({"id": "org.test.cycle"})).unwrap()["removed"], false);
    // Still listed by the catalog, no longer installed.
    assert!(package(&s.execute("library.list", json!({})).unwrap(), "org.test.cycle")["installed"].is_null());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_checksum_or_a_bad_module_installs_nothing() {
    let dir = tmp("checks");
    let (url, _) = serve(&dir, "a.wasm", "org.test.library.checks.a");
    let garbage = dir.join("served").join("garbage.wasm");
    std::fs::write(&garbage, b"\0asm\x01\0\0\0garbage").unwrap();
    let garbage_sha = sha256::hex(&std::fs::read(&garbage).unwrap());
    let catalog = write_catalog(
        &dir,
        json!([
            {"id": "org.test.badsum", "name": "Bad Sum", "versions": [{"version": "1", "api": 1, "url": url, "sha256": "0".repeat(64)}]},
            {"id": "org.test.garbage", "name": "Garbage", "versions": [{"version": "1", "api": 1, "url": file_url(&garbage), "sha256": garbage_sha}]},
            {"id": "org.test.web", "name": "Web", "versions": [{"version": "1", "api": 1, "url": "https://example.com/x.wasm", "sha256": "0".repeat(64)}]},
            {"id": "org.test.future", "name": "Future", "versions": [{"version": "1", "api": 99, "url": url, "sha256": "0".repeat(64)}]}
        ]),
    );
    let mut s = session(&dir);
    s.execute("library.refresh", json!({"catalog": catalog, "wait": true})).unwrap();
    let e = s.execute("library.install", json!({"id": "org.test.badsum", "wait": true})).unwrap_err().to_string();
    assert!(e.contains("SHA-256"), "{e}");
    let e = s.execute("library.install", json!({"id": "org.test.garbage", "wait": true})).unwrap_err().to_string();
    assert!(e.contains("didn't load"), "{e}");
    let e = s.execute("library.install", json!({"id": "org.test.web", "wait": true})).unwrap_err().to_string();
    assert!(e.contains("isn't on GitHub"), "{e}");
    let e = s.execute("library.install", json!({"id": "org.test.future", "wait": true})).unwrap_err().to_string();
    assert!(e.contains("no release for this EffectCraft"), "{e}");
    assert_eq!(package(&s.execute("library.list", json!({})).unwrap(), "org.test.future")["needsNewerApp"], true);
    let lib = dir.join("Plug-ins").join("Library");
    for id in ["org.test.badsum", "org.test.garbage", "org.test.web"] {
        assert!(!lib.join(id).exists(), "{id}");
    }
    assert!(!lib.join("installed.json").exists());
    // Remote catalogs must be on GitHub too; the list still works from the cached copy.
    assert!(s.execute("library.refresh", json!({"catalog": "https://example.com/index.json"})).unwrap_err().to_string().contains("isn't on GitHub"));
    assert!(s.execute("library.refresh", json!({"catalog": "http://raw.githubusercontent.com/x"})).is_err());
    let mut fresh = session(&dir);
    assert_eq!(fresh.execute("library.list", json!({})).unwrap()["packages"].as_array().unwrap().len(), 4, "read from catalog.json");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn installed_packages_load_at_start_and_changed_files_are_refused() {
    let dir = tmp("startup");
    let lib = dir.join("Plug-ins").join("Library");
    let mut entries = serde_json::Map::new();
    for (id, effect, tamper) in [("org.test.start.ok", "org.test.library.start.ok", false), ("org.test.start.tampered", "org.test.library.start.tampered", true)] {
        let bytes = wat_plugin(effect, 1, "i32.const 0").into_bytes();
        let file = format!("{id}/1.0.0/{id}.wasm");
        let path = lib.join(id).join("1.0.0");
        std::fs::create_dir_all(&path).unwrap();
        let written = if tamper { wat_plugin(effect, 1, "i32.const 1").into_bytes() } else { bytes.clone() };
        std::fs::write(path.join(format!("{id}.wasm")), written).unwrap();
        entries.insert(id.into(), json!({"version": "1.0.0", "file": file, "sha256": sha256::hex(&bytes), "effects": [effect]}));
    }
    entries.insert("org.test.start.escape".into(), json!({"version": "1", "file": "../../outside.wasm", "sha256": "0".repeat(64)}));
    std::fs::write(lib.join("installed.json"), json!({"packages": entries}).to_string()).unwrap();
    let mut s = session(&dir);
    let r = s.execute("library.loadInstalled", json!({})).unwrap();
    let loaded: Vec<&str> = r["loaded"].as_array().unwrap().iter().filter_map(|v| v["package"].as_str()).collect();
    assert_eq!(loaded, ["org.test.start.ok"], "{r}");
    let errors = r["errors"].to_string();
    assert!(errors.contains("org.test.start.tampered") && errors.contains("SHA-256"), "{r}");
    assert!(errors.contains("bad module path"), "{r}");
    assert!(plugin_ids(&mut s).iter().any(|i| i == "org.test.library.start.ok"));
    // Running it again loads nothing twice.
    assert_eq!(s.execute("library.loadInstalled", json!({})).unwrap()["loaded"], json!([]));
    // Without a catalog the installed packages are still listed.
    let l = s.execute("library.list", json!({})).unwrap();
    let p = package(&l, "org.test.start.ok");
    assert_eq!((p["unlisted"].as_bool(), p["loaded"].as_str()), (Some(true), Some("1.0.0")), "{l}");
    // A corrupt installed.json is an empty library, not a failure.
    std::fs::write(lib.join("installed.json"), b"{not json").unwrap();
    assert_eq!(session(&dir).execute("library.loadInstalled", json!({})).unwrap()["loaded"], json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_effects_name_the_packages_that_provide_them() {
    let dir = tmp("missing");
    let fx = "org.test.library.missing.fx";
    let v = serve(&dir, "m.wasm", fx);
    let catalog = write_catalog(&dir, json!([{"id": "org.test.missing", "name": "Missing", "versions": [release("1.0.0", &v, fx)]}]));
    let mut s = session(&dir);
    s.execute("library.refresh", json!({"catalog": catalog, "wait": true})).unwrap();
    // A project whose effects include one this session doesn't have (as when a project from
    // someone with the plug-in opens here).
    s.execute("comp.new", json!({"name": "M", "width": 8, "height": 8, "duration": 1})).unwrap();
    let layer = s.execute("layer.newSolid", json!({"color": [1, 0, 0], "width": 8, "height": 8})).unwrap()["layer"].as_u64().unwrap();
    s.execute("effect.apply", json!({"layer": layer, "effect": "ec.blur.gaussian"})).unwrap();
    s.execute("effect.apply", json!({"layer": layer, "effect": "ec.blur.gaussian"})).unwrap();
    let cid = s.active_comp_id().unwrap();
    let proj = std::sync::Arc::make_mut(&mut s.project);
    let l = proj.comp_mut(cid).unwrap().layer_mut(LayerId(layer)).unwrap();
    let first = l.props.sub_mut("effects").unwrap().children.iter_mut().find_map(|n| match n {
        Node::Group(g) => Some(g),
        _ => None,
    });
    first.unwrap().kind = GroupKind::Effect { effect: fx.into() };
    let r = s.execute("library.missing", json!({})).unwrap();
    assert_eq!(r["effects"], json!([{"effect": fx, "packages": ["org.test.missing"]}]), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}
