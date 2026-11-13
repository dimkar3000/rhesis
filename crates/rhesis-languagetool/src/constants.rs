//! Worker-crate constants: tuning knobs, fallback values and the fixture
//! seam identifiers.

/// How many consecutive start attempts may fail before the worker gives up
/// and latches into `Failed`. Reset by `Stop` and `Retry`.
pub const MAX_START_FAILURES: u32 = 3;

/// The color a recommendation gets when neither its rule id nor its
/// category id matches an entry of the user's color table: the default red
/// the UI pipeline and the recorded fixtures assume.
pub(crate) const DEFAULT_MATCH_COLOR: &str = "#FF0000";

/// LanguageTool API endpoint text checks are posted to (the light
/// health probe uses the same path), appended to the normalized base
/// address: `<base>/<CHECK_PATH>`.
pub(crate) const CHECK_PATH: &str = "v2/check";

/// (fasttext model, fasttext binary) locations relative to the LanguageTool
/// dir, covering every layout that does not already ship a server.properties:
/// dev artifacts (setup.sh), desktop payloads, Flatpak.
pub(crate) const FASTTEXT_LAYOUTS: &[(&str, &str)] = &[
    ("../lid.176.ftz", "../fastText/fasttext"),
    ("../lid.176.ftz", "../bin/fasttext"),
    ("../lid.176.ftz", "../../bin/fasttext"),
];
