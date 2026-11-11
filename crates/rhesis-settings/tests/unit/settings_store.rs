//! Settings-store tier (Refactor.md §9): defaults, atomic writes and
//! corrupt-file quarantine — against real store files in per-test
//! sandboxes, never the live user configuration.

use std::path::PathBuf;

use rhesis_constants::DEFAULT_PORT;
use rhesis_settings::settings::Settings;
use rhesis_settings::store::Store;

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rhesis-settings-store-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn store_in(root: &std::path::Path) -> Store {
    Store::open(root.join("service.json"))
}

#[test]
fn first_load_materializes_defaults_and_persists() {
    let root = sandbox("fresh");
    let store = store_in(&root);

    // First load: pure defaults, no panic.
    assert_eq!(store.load(), Settings::default());

    let path = root.join("service.json");
    let raw = std::fs::read_to_string(&path).unwrap();
    let disk: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(disk["schema_version"], 1);
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
    let store = store_in(&root);
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

    let store = store_in(&root);
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
    assert!(path.is_file(), "the store is rewritten with defaults");

    let store = store_in(&root);
    assert_eq!(store.load(), Settings::default(), "the next load is clean");
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
