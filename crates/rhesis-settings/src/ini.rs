//! Minimal QSettings-INI reader for the one-time settings migration.
//!
//! Scope is deliberately narrow: the pre-service UI wrote exactly one
//! category-less QML `Settings` store (all keys under `[General]`, verified
//! on real machines), so the migration only consults that section; other
//! groups are parsed for completeness but never migrated. Values needed are
//! plain QSettings strings plus one binary shape: the `@Variant(...)` blob
//! QSettings writes for `QVariantMap` values (the color table).
//!
//! The `@Variant` decoding is validated byte-for-byte against blobs produced
//! by real Qt (see the fixtures and the in-module tests).

use std::collections::BTreeMap;
use std::path::Path;

use rhesis_protocol::constants::DEFAULT_PORT;
use rhesis_protocol::Settings;

/// A parsed INI: section names lowercased (QSettings matches them without
/// case), keys and values as written (values unescaped on demand).
#[derive(Debug, Default, PartialEq)]
pub struct ParsedIni {
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl ParsedIni {
    fn section_mut(&mut self, name: &str) -> &mut BTreeMap<String, String> {
        let lowercase = name.to_lowercase();
        self.sections.entry(lowercase).or_default()
    }

    /// The `[General]` section, or nothing when the file has none.
    pub fn general(&self) -> &BTreeMap<String, String> {
        static EMPTY: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
        self.sections
            .get("general")
            .unwrap_or_else(|| EMPTY.get_or_init(BTreeMap::new))
    }
}

/// Read and parse a QSettings INI file. Syntactically broken escape
/// sequences inside values stay as raw text: the per-key value parsers
/// decide what they can still use, and unknown keys are ignored anyway.
pub fn read_ini(path: &Path) -> std::io::Result<ParsedIni> {
    let contents = std::fs::read_to_string(path)?;
    Ok(parse_ini(&contents))
}

/// Parse INI text. Line-oriented: ``[section]`` headers, `key=value` lines
/// (first `=` separates), `;`/`#` whole-line comments, headerless leading
/// keys fold into `[General]` like QSettings itself would.
pub fn parse_ini(contents: &str) -> ParsedIni {
    let mut ini = ParsedIni {
        sections: BTreeMap::new(),
    };
    let mut current = "general";
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].trim();
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            let value = line[eq + 1..].trim().to_string();
            if !key.is_empty() {
                ini.section_mut(current).insert(key, value);
            }
        }
    }
    ini
}

/// Build the settings snapshot from the legacy INI: known keys override
/// defaults, stale keys (`defaultPort`, `colorRules`, `remoteUrl`) and
/// out-of-`[General]` groups are ignored. Undecodable values fall back to
/// their QML-era defaults with a warning — a half-broken store never loses
/// its other keys. `warn` exists so the CLI-free crate stays silent by
/// default while tests can spy on the warnings.
pub fn settings_from_ini(ini: &ParsedIni, warn: &mut dyn FnMut(&str)) -> Settings {
    let mut settings = Settings::default();

    for (key, value) in ini.general() {
        match key.as_str() {
            k if k.eq_ignore_ascii_case("embedded") => {
                settings.embedded = parse_bool_value(value, true, k, warn);
            }
            k if k.eq_ignore_ascii_case("background") => {
                settings.background = parse_bool_value(value, false, k, warn);
            }
            k if k.eq_ignore_ascii_case("showTooltips") => {
                settings.show_tooltips = parse_bool_value(value, true, k, warn);
            }
            k if k.eq_ignore_ascii_case("showDebugTooltips") => {
                settings.show_debug_tooltips = parse_bool_value(value, true, k, warn);
            }
            k if k.eq_ignore_ascii_case("port") => {
                settings.port = match value.trim().parse::<u16>() {
                    Ok(port) => port,
                    Err(_) => {
                        warn(&format!(
                            "migration: port={value:?} is not a port, using the default"
                        ));
                        DEFAULT_PORT
                    }
                };
            }
            k if k.eq_ignore_ascii_case("externalAddress") => match decode_plain_string(value) {
                Some(address) => settings.external_address = address,
                None => {
                    warn(&format!(
                        "migration: externalAddress={value:?} failed to decode, using empty"
                    ));
                }
            },
            k if k.eq_ignore_ascii_case("language") => {
                settings.language = decode_plain_string(value).unwrap_or_default();
            }
            k if k.eq_ignore_ascii_case("colorSettings") => match qvariant_string_map(value) {
                Some(map) if !map.is_empty() => settings.color_settings = map,
                _ => warn(
                    "migration: colorSettings was not a decodable string map, \
                     keeping the default colors",
                ),
            },
            _ => (), // unknown/stale keys die with the old store
        }
    }
    settings
}

/// `true`/`false` or the fallback with a warning (QML wrote plain bools).
fn parse_bool_value(raw: &str, fallback: bool, key: &str, warn: &mut dyn FnMut(&str)) -> bool {
    match decode_plain_string(raw) {
        Some(v) if v == "true" => true,
        Some(v) if v == "false" => false,
        _ => {
            warn(&format!(
                "migration: {key}={raw:?} is not a bool, using the default"
            ));
            fallback
        }
    }
}

/// Named escape table, byte-identical to `iniUnescapedStringList` in Qt's
/// qsettings.cpp (`\0` is spelled via the octal branch there; same byte).
fn named_escape(c: char) -> Option<u8> {
    let byte = match c {
        'a' => 0x07,
        'b' => 0x08,
        'f' => 0x0C,
        'n' => 0x0A,
        'r' => 0x0D,
        't' => 0x09,
        'v' => 0x0B,
        '0' => 0x00,
        '"' => b'"',
        '?' => b'?',
        '\'' => b'\'',
        '\\' => b'\\',
        _ => return None,
    };
    Some(byte)
}

/// The `\x` escape the QSettings reader understands: one or two hex digits,
/// minimal length (`\xe` and `\x43` are what the writer emits — never
/// zero-padded). Consumes digits from `chars` (positioned after the `x`).
fn hex_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<u8> {
    let hi = chars.next()?.to_digit(16)?;
    let mut value = hi;
    // Greedy second digit: the reader also eats two; a writer byte ≥ 0x10
    // always shows as two digits, a byte < 0x10 as one.
    if let Some(lo) = chars.peek().and_then(|&c| c.to_digit(16)) {
        value = value * 16 + lo;
        chars.next();
    }
    u8::try_from(value).ok()
}

/// Decode a QSettings plain string value: optional surrounding quotes, then
/// the escape set of `iniEscapedString`/`iniUnescapedStringList` in Qt's
/// qsettings.cpp: the named table (`\a \b \f \n \r \t \v \" \' \? \\ \0`),
/// `\xHH` with minimal-length digits, anything else leniently passed
/// through (the established migration decision: the escape letters the
/// format grows over time must not lose our ASCII keys). Non-ASCII text
/// stays its Unicode self — these values are only ever true/false, a port,
/// a URL or a locale code.
pub fn decode_plain_string(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let inner = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let mut decoded = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            decoded.push(c);
            continue;
        }
        let escape = chars.next()?;
        if let Some(named) = named_escape(escape) {
            decoded.push(char::from(named));
        } else if escape == 'x' {
            decoded.push(char::from(hex_escape(&mut chars)?));
        } else {
            // Unknown escapes are literal (lenient pass-through).
            decoded.push(escape);
        }
    }
    Some(decoded)
}

/// The `@Variant(...)` shape QSettings writes for `QVariantMap` values,
/// decoded to its string→string pairs. Only the map-of-strings shape the
/// color table ever produced is accepted; anything else (other container
/// markers, non-string values, odd lengths, trailing bytes) fails the whole
/// value so the caller keeps the defaults.
///
/// Byte layout of the inner blob, validated against real Qt 6 output:
/// `[u32 BE = 8 (QVariantMap type marker)][u32 BE count]`, then per pair
/// `[QString key: u32 BE len + UTF-16BE data][QVariant value: u32 BE typeId
/// = 10 (QString) + u32 BE len + UTF-16BE data]`.
pub fn qvariant_string_map(raw: &str) -> Option<BTreeMap<String, String>> {
    let raw = raw.trim();
    let inner = raw
        .strip_prefix("@Variant(")
        .and_then(|rest| rest.strip_suffix(')'))?;
    let bytes = variant_payload_bytes(inner)?;
    let mut cursor = &bytes[..];

    let marker = q_u32(&mut cursor)?;
    if marker != 8 {
        // QVariantMap marker; other container types are unknown shapes.
        return None;
    }
    let count = q_u32(&mut cursor)?;
    if count > 1024 {
        return None;
    }

    let mut map = BTreeMap::new();
    for _ in 0..count {
        let key = q_string(&mut cursor)?;
        let value_type = q_u32(&mut cursor)?;
        if value_type != 10 {
            // Every color entry is a QString; other elements would mean a
            // producer this code never saw.
            return None;
        }
        let value = q_string(&mut cursor)?;
        map.insert(key, value);
    }
    if !cursor.is_empty() {
        // Trailing bytes: not the shape this migration was written against.
        return None;
    }
    Some(map)
}

/// BE `u32` taken from the front of `cursor`, which then advances.
fn q_u32(cursor: &mut &[u8]) -> Option<u32> {
    let (head, rest) = cursor.split_first_chunk::<4>()?;
    *cursor = rest;
    Some(u32::from_be_bytes(*head))
}

/// QString payload on the front of `cursor`: BE `u32` byte length plus
/// UTF-16BE characters.
fn q_string(cursor: &mut &[u8]) -> Option<String> {
    let length = q_u32(cursor)? as usize;
    if length > 32768 || !length.is_multiple_of(2) || cursor.len() < length {
        return None;
    }
    let (data, rest) = cursor.split_at(length);
    *cursor = rest;
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// The escape-encoded text between `@Variant(` `)` becomes raw bytes.
/// Mirrors `iniUnescapedStringList` from Qt's qsettings.cpp on the shapes
/// its writer (`iniEscapedString`) actually produces: the named table, `\x`
/// with minimal-length hex digits (one or two), and single-byte octal runs.
/// Unknown escape letters fail the whole value (strict, unlike plain
/// strings — a mis-stropped binary blob would corrupt the byte alignment,
/// and only ever means a producer this decoder was not written against).
fn variant_payload_bytes(inner: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(inner.len() / 2);
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        let byte = match c {
            '\\' => {
                let escape = chars.next()?;
                match named_escape(escape) {
                    Some(named) => Some(named),
                    None if escape == 'x' => hex_escape(&mut chars),
                    None if ('0'..='7').contains(&escape) => {
                        // Octal run: `\0` is the only form the writer emits,
                        // extended greedily exactly like the reader does.
                        let mut value = escape.to_digit(8).expect("octal digit");
                        for _ in 0..2 {
                            match chars.peek().and_then(|&next| next.to_digit(8)) {
                                Some(digit) => {
                                    value = value * 8 + digit;
                                    chars.next();
                                }
                                None => break,
                            }
                        }
                        u8::try_from(value).ok()
                    }
                    // Unknown escape: refuse the value, keep the defaults.
                    None => None,
                }
            }
            literal if literal.is_ascii() => u8::try_from(u32::from(literal)).ok(),
            // Non-ASCII inside the blob never occurs (the writer emits a
            // Latin-1 view of the stream).
            _ => None,
        }?;
        bytes.push(byte);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blob real Qt writes for the default color map (captured from a
    /// `QSettings` generator run, stored verbatim).
    const DEFAULT_COLORS_BLOB: &str = r"@Variant(\0\0\0\b\0\0\0\x3\0\0\0 \0\x43\0\x41\0T\0\x45\0G\0O\0R\0Y\0:\0G\0R\0\x41\0M\0M\0\x41\0R\0\0\0\n\0\0\0\xe\0#\0\x65\0\x63\0\x63\0\x32\0\x32\0\x34\0\0\0&\0\x43\0\x41\0T\0\x45\0G\0O\0R\0Y\0:\0T\0Y\0P\0O\0G\0R\0\x41\0P\0H\0Y\0\0\0\n\0\0\0\xe\0#\0\x66\0\x36\0\x33\0\x65\0\x66\0\x39\0\0\0\x1c\0\x43\0\x41\0T\0\x45\0G\0O\0R\0Y\0:\0T\0Y\0P\0O\0S\0\0\0\n\0\0\0\xe\0#\0\x31\0\x61\0\x65\0\x39\0\x31\0\x38)";

    #[test]
    fn converts_a_real_qvariant_map_blob() {
        let map = qvariant_string_map(DEFAULT_COLORS_BLOB).expect("real blob must decode");
        assert_eq!(
            map,
            BTreeMap::from([
                ("CATEGORY:GRAMMAR".to_string(), "#ecc224".to_string()),
                ("CATEGORY:TYPOGRAPHY".to_string(), "#f63ef9".to_string()),
                ("CATEGORY:TYPOS".to_string(), "#1ae918".to_string()),
            ])
        );
    }

    #[test]
    fn non_string_or_truncated_map_shapes_fail() {
        // Container marker other than 8 (QVariantMap).
        assert!(qvariant_string_map("@Variant(\\0\\0\\0\\x9)").is_none());
        // Count announces more pairs than bytes remain.
        assert!(qvariant_string_map("@Variant(\\0\\0\\0\\b\\0\\0\\0\\x2)").is_none());
        // Empty blob: no marker at all.
        assert!(qvariant_string_map("@Variant()").is_none());
    }

    #[test]
    fn empty_maps_decode_but_carry_nothing() {
        let map = qvariant_string_map("@Variant(\\0\\0\\0\\b\\0\\0\\0\\0)")
            .expect("a zero-pair blob is a valid map");
        assert!(map.is_empty());
    }

    #[test]
    fn escapes_decode_in_plain_strings() {
        assert_eq!(decode_plain_string("el_GR").as_deref(), Some("el_GR"));
        assert_eq!(decode_plain_string("\"a=b;c\"").as_deref(), Some("a=b;c"));
        assert_eq!(decode_plain_string("a\\=b").as_deref(), Some("a=b"));
        assert_eq!(
            decode_plain_string("line\\nbreak").as_deref(),
            Some("line\nbreak")
        );
        assert_eq!(decode_plain_string("\\x41BC").as_deref(), Some("ABC"));
        assert!(decode_plain_string("").is_some());
        // Unknown escapes pass their character through.
        assert_eq!(decode_plain_string("sd\\qf").as_deref(), Some("sdqf"));
    }

    #[test]
    fn settings_from_ini_uses_known_keys_and_stability() {
        let ini = parse_ini(
            r#"
; stale-era store with comments
[General]
embedded = false
port=7654
externalAddress=http://192.168.1.10:8081
background=true
showTooltips=false
showDebugTooltips=true
language=el_GR
remoteUrl=
defaultPort=2689
[Other]
embedded=true
"#,
        );
        let settings = settings_from_ini(&ini, &mut |_| {});
        assert!(!settings.embedded, "[General] wins over other sections");
        assert_eq!(settings.port, 7654);
        assert_eq!(settings.external_address, "http://192.168.1.10:8081");
        assert!(settings.background);
        assert!(!settings.show_tooltips);
        assert!(settings.show_debug_tooltips);
        assert_eq!(settings.language, "el_GR");
        // Defaults survive when no key overrides them (no color map here).
        assert_eq!(settings.color_settings, Settings::default().color_settings);
    }

    #[test]
    fn broken_values_fall_back_with_a_warning() {
        let ini = parse_ini("[General]\nembedded=perhaps\nport=76545\nbackground=true\n");
        let mut warnings = Vec::new();
        let mut warn = |w: &str| warnings.push(w.to_string());
        let settings = settings_from_ini(&ini, &mut warn);
        assert!(settings.embedded, "not a bool");
        assert_eq!(settings.port, DEFAULT_PORT, "out of range -> default");
        assert!(settings.background);
        assert_eq!(warnings.len(), 2, "each broken value warns");
    }
}
