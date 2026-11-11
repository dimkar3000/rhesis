//! Settings-store tier (Refactor.md §9): defaults, atomic writes, corrupt
//! handling, and the one-time INI migration — driven against a QSettings
//! fixture produced by real Qt and copied per-test so the fixture itself is
//! never mutated. Store paths always live in a per-run sandbox, and every
//! migration input is pinned through `with_ini_path`: no live user
//! configuration is touched (hermeticity by construction).

use std::path::PathBuf;

use rhesis_constants::DEFAULT_PORT;
use rhesis_settings::settings::Settings;
use rhesis_settings::store::Store;

/// Real-QSettings-written legacy INI fixture (see tests/fixtures/ini/).
const FIXTURE_INI: &str = "qml_store.conf";

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rhesis-settings-store-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A store with the INI override pinned to an explicit path — plain
/// `Store::open` alone would resolve through the real user configuration,
/// which is exactly what these tiers must not touch.
fn store_in(root: &std::path::Path, ini_path: Option<PathBuf>) -> Store {
    Store::open(root.join("service.json")).with_ini_path(ini_path)
}

/// Copy the real-QSettings fixture into the sandbox and pin the store to it.
fn fixture_ini(root: &std::path::Path) -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = manifest.join("tests/fixtures/ini").join(FIXTURE_INI);
    let target = root.join(FIXTURE_INI);
    std::fs::copy(&source, &target).expect("fixture copied");
    Some(target)
}

/// An INI path that never exists: the store stays un-migrated.
fn absent_ini(root: &std::path::Path) -> Option<PathBuf> {
    Some(root.join("absent-qsettings.ini"))
}

#[test]
fn fresh_store_seeds_defaults_and_persists() {
    let root = sandbox("fresh");
    let store = store_in(&root, absent_ini(&root));

    // First load: pure defaults (no INI anywhere), no panic, marker false.
    assert_eq!(store.load(), Settings::default());

    let path = root.join("service.json");
    let raw = std::fs::read_to_string(&path).unwrap();
    let disk: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(disk["schema_version"], 1);
    assert_eq!(disk["migrated_v1"], false);
    assert_eq!(disk["settings"]["port"], DEFAULT_PORT);
    assert_eq!(disk["settings"]["embedded"], true);
    assert_eq!(
        disk["settings"]["colorSettings"]["CATEGORY:GRAMMAR"], "#ecc224",
        "JSON keys keep the QML-era names"
    );

    // Second load: stable (returns the same values from the file).
    assert_eq!(store.load(), Settings::default());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn saves_are_atomic_pretty_and_stable() {
    let root = sandbox("atomic");
    let store = store_in(&root, absent_ini(&root));
    store.load();

    let modified = Settings {
        embedded: false,
        port: 7654,
        external_address: "http://192.168.1.10:8081".to_string(),
        language: "el_GR".to_string(),
        ..Settings::default()
    };
    store.save(&modified).expect("save succeeds");

    // Tmp file is gone; only the final store remains.
    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|entry| entry.unwrap().file_name().into_string().ok())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "tmp leftovers: {leftovers:?}");

    // Save/load keeps ints ints and strings strings (the store is the
    // single file both ends diff against).
    let raw = std::fs::read_to_string(root.join("service.json")).unwrap();
    assert!(raw.contains("\"port\": 7654"), "port as a number: {raw}");
    assert!(
        raw.contains("\"language\": \"el_GR\""),
        "language as a string"
    );

    let reloaded = store.load();
    assert_eq!(reloaded, modified);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn corrupt_store_is_quarantined_not_deleted() {
    let root = sandbox("corrupt");
    let path = root.join("service.json");
    std::fs::write(&path, "{ this is not json [[").unwrap();

    let store = store_in(&root, absent_ini(&root));
    assert_eq!(store.load(), Settings::default());

    let quarantined = path.with_extension("json.corrupt");
    assert!(
        quarantined.is_file(),
        "the corrupt file is renamed aside, not deleted"
    );
    assert_eq!(
        std::fs::read_to_string(&quarantined).unwrap(),
        "{ this is not json [[",
        "the bytes remain diagnosable after quarantine"
    );
    assert!(path.is_file(), "the store is rewritten from the seed");

    let store = store_in(&root, absent_ini(&root));
    assert_eq!(store.load(), Settings::default(), "the next load is clean");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn migration_runs_once_from_the_legacy_ini() {
    let root = sandbox("migrate");
    let store = store_in(&root, fixture_ini(&root));

    let settings = store.load();
    // Values straight out of the real-QSettings fixture.
    assert!(!settings.embedded);
    assert_eq!(settings.port, 7654);
    assert_eq!(settings.external_address, "http://192.168.1.10:8081");
    assert!(settings.background);
    assert!(!settings.show_tooltips);
    assert!(settings.show_debug_tooltips);
    assert_eq!(settings.language, "el_GR");
    // The colors came through the @Variant blob, custom rule included;
    // stale keys (colorRules, remoteUrl, defaultPort) do not migrate.
    assert_eq!(
        settings.color_settings,
        [
            ("CATEGORY:GRAMMAR", "#ecc224"),
            ("CATEGORY:TYPOGRAPHY", "#f63ef9"),
            ("CATEGORY:TYPOS", "#1ae918"),
            ("RULE:MY_RULE", "#101010"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
        "exactly the fixture's color map, nothing stale"
    );

    // The seed persisted, with migrated_v1 = true.
    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("service.json")).unwrap()).unwrap();
    assert_eq!(disk["migrated_v1"], true);
    assert_eq!(disk["settings"]["port"], 7654);

    // The INI is never rewritten by the migration (rollback stays possible).
    assert_eq!(
        std::fs::read_to_string(root.join(FIXTURE_INI)).unwrap(),
        std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/ini")
                .join(FIXTURE_INI)
        )
        .unwrap(),
        "migration leaves the INI untouched"
    );

    // Next loads read the store file only (the INI could disappear/change).
    std::fs::write(root.join(FIXTURE_INI), "broken").unwrap();
    assert_eq!(
        store.load(),
        settings,
        "the store file wins over whatever the INI says now"
    );

    // Even with the JSON reset, a now-broken INI degrades to defaults.
    std::fs::remove_file(root.join("service.json")).unwrap();
    let store = store_in(&root, Some(root.join(FIXTURE_INI)));
    assert_eq!(
        store.load(),
        Settings::default(),
        "broken INI seeds defaults"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn json_shadows_the_legacy_ini_from_the_first_byte() {
    let root = sandbox("shadow");
    // Store exists from a prior run; INI present with different values.
    let existing = Settings {
        port: 1111,
        ..Settings::default()
    };
    let ini_path = fixture_ini(&root);
    store_in(&root, ini_path.clone())
        .save(&existing)
        .expect("save succeeds");

    let loaded = store_in(&root, ini_path.clone()).load();
    assert_eq!(loaded.port, 1111, "the JSON store wins over the INI");
    assert_eq!(loaded.external_address, "", "INI values are not merged");

    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("service.json")).unwrap()).unwrap();
    assert_eq!(disk["migrated_v1"], false, "no re-migration: the JSON wins");
    assert_eq!(
        disk["settings"]["externalAddress"], "",
        "real legacy camelCase keys, not struct snake_case"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicitly_absent_ini_seeds_plain_defaults() {
    let root = sandbox("no-ini");
    // An override pointing at a file that never exists: no INI seed.
    let store = store_in(&root, absent_ini(&root));
    assert_eq!(store.load(), Settings::default());
    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("service.json")).unwrap()).unwrap();
    assert_eq!(disk["migrated_v1"], false);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unreadable_ini_falls_back_to_defaults() {
    let root = sandbox("bad-ini");
    std::fs::write(
        root.join("broken-qsettings.ini"),
        b"\x00\xff\xfe broken utf8",
    )
    .unwrap();
    let store = store_in(&root, Some(root.join("broken-qsettings.ini")));
    assert_eq!(
        store.load(),
        Settings::default(),
        "unreadable INI seed = defaults"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn transient_store_never_touches_the_disk() {
    let root = sandbox("transient");
    let store = Store::transient();

    assert_eq!(
        store.load(),
        Settings::default(),
        "defaults in, defaults out"
    );
    assert!(matches!(
        store.save(&Settings::default()),
        Err(rhesis_settings::store::SaveError::Transient)
    ));
    assert!(
        !root.join("service.json").is_file(),
        "a transient store writes no file anywhere"
    );
}
