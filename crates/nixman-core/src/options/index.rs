//! Build an [`OptionIndex`] by evaluating the NixOS option set via `nix build`.
//!
//! The entry point is [`build`], which:
//!
//! 1. Verifies that `flake.lock` is present.
//! 2. Discovers which `nixosConfigurations` hostname to use.
//! 3. Runs `nix build <flake>#nixosConfigurations.<host>.config.system.build.manual.optionsJSON`
//!    to produce the standard NixOS `options.json`.
//! 4. Parses and returns the result as an [`OptionIndex`].
//!
//! Progress is reported to the caller via a `std::sync::mpsc::Sender<f32>`
//! with values 0.0 → 0.5 (after build) → 1.0 (after parse).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;

use chrono::Utc;
use serde::Deserialize;

use crate::options::cache;
use crate::options::types::{OptionIndex, OptionMeta, OptionType};

// IndexError

/// Errors that can occur while building the option index.
#[derive(Debug)]
pub enum IndexError {
    /// `nix build` (or a supporting `nix eval`) exited non-zero.
    NixBuildFailed(String),
    /// JSON or structured data could not be parsed.
    ParseError(String),
    /// No `flake.lock` file was found in the given directory.
    FlakeLockNotFound,
    /// An I/O error.
    IoError(io::Error),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexError::NixBuildFailed(msg) => write!(f, "nix build failed: {}", msg),
            IndexError::ParseError(msg) => write!(f, "parse error: {}", msg),
            IndexError::FlakeLockNotFound => write!(f, "flake.lock not found"),
            IndexError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IndexError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for IndexError {
    fn from(e: io::Error) -> Self {
        IndexError::IoError(e)
    }
}

// Raw JSON shape — standard NixOS options.json format

/// One entry in the NixOS `options.json` file.
///
/// Fields are parsed as `serde_json::Value` where the shape is variable
/// (e.g. `default` can be a primitive, a `{ _type, text }` wrapper, or null).
#[derive(Deserialize)]
struct RawOption {
    /// List of source files that declare this option.
    /// Each element may be a bare string or a `{ name, column, line }` object
    /// depending on the NixOS version, so we keep it as `Value`.
    #[serde(default)]
    declarations: Vec<serde_json::Value>,
    /// Human-readable description; may be a string or `{ _type, text }`.
    #[serde(default)]
    description: serde_json::Value,
    /// Default value; may be null, a primitive, or a `{ _type, text }` wrapper.
    #[serde(default)]
    default: serde_json::Value,
    /// Example value; same shape as `default`.
    #[serde(default)]
    example: serde_json::Value,
    /// Type string, e.g. `"boolean"`, `"list of string"`.
    #[serde(rename = "type", default)]
    type_str: String,
}


/// Evaluate the full NixOS option set for the flake at `flake_path` and
/// return a populated [`OptionIndex`].
///
/// # Progress
///
/// Approximate progress values sent via `progress_tx`:
/// - `0.0` — immediately on entry
/// - `0.5` — after `nix build` completes
/// - `1.0` — after parsing is complete
///
/// Sending failures (i.e. when the receiver has been dropped) are silently
/// ignored.
pub fn build(flake_path: &Path, progress_tx: mpsc::Sender<f32>) -> Result<OptionIndex, IndexError> {
    let _ = progress_tx.send(0.0);

    let lock_path = flake_path.join("flake.lock");
    if !lock_path.exists() {
        return Err(IndexError::FlakeLockNotFound);
    }

    let flake_lock_hash = cache::hash_flake_lock(flake_path).map_err(|e| {
        IndexError::IoError(io::Error::new(io::ErrorKind::Other, e.to_string()))
    })?;

    let nixpkgs_rev = read_nixpkgs_rev(flake_path)?;

    let hostname = find_hostname(flake_path)?;

    let out_link = nix_build_options(flake_path, &hostname)?;
    let _ = progress_tx.send(0.5);

    let options_json = out_link
        .join("share")
        .join("doc")
        .join("nixos")
        .join("options.json");

    let options = parse_options_json(&options_json)?;

    let _ = fs::remove_file(&out_link);

    let _ = progress_tx.send(1.0);

    Ok(OptionIndex {
        options,
        flake_lock_hash,
        built_at: Utc::now(),
        nixpkgs_rev,
    })
}


/// Read the nixpkgs git revision from `flake.lock`.
///
/// Tries common node names (`nixpkgs`, `nixpkgs-stable`, …) and falls back
/// to any node whose key contains "nixpkgs".  Returns `"unknown"` if nothing
/// matches.
fn read_nixpkgs_rev(flake_path: &Path) -> Result<String, IndexError> {
    let lock_path = flake_path.join("flake.lock");
    let content = fs::read_to_string(&lock_path)?;

    let lock: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| IndexError::ParseError(format!("failed to parse flake.lock: {}", e)))?;

    let nodes = match lock.get("nodes").and_then(|n| n.as_object()) {
        Some(n) => n,
        None => return Ok("unknown".to_string()),
    };

    // Try well-known node names first.
    for name in &["nixpkgs", "nixpkgs-stable", "nixos-stable", "nixos-unstable"] {
        if let Some(rev) = nodes
            .get(*name)
            .and_then(|n| n.get("locked"))
            .and_then(|n| n.get("rev"))
            .and_then(|v| v.as_str())
        {
            return Ok(rev.to_string());
        }
    }

    // Fallback: scan any node whose key contains "nixpkgs".
    for (key, val) in nodes {
        if key.to_lowercase().contains("nixpkgs") {
            if let Some(rev) = val
                .get("locked")
                .and_then(|n| n.get("rev"))
                .and_then(|v| v.as_str())
            {
                return Ok(rev.to_string());
            }
        }
    }

    Ok("unknown".to_string())
}

/// Discover the `nixosConfigurations` hostname to build.
///
/// Strategy:
/// 1. Enumerate `nixosConfigurations` attribute names via `nix eval`.
/// 2. Try to match the current system hostname.
/// 3. Fall back to the first name in the list.
fn find_hostname(flake_path: &Path) -> Result<String, IndexError> {
    let flake_ref = flake_path.to_string_lossy();
    let attr = format!("{}#nixosConfigurations", flake_ref);

    let output = Command::new("nix")
        .args(["eval", &attr, "--apply", "builtins.attrNames", "--json"])
        .output()
        .map_err(IndexError::IoError)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(IndexError::NixBuildFailed(format!(
            "failed to list nixosConfigurations: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let names: Vec<String> = serde_json::from_str(&stdout).map_err(|e| {
        IndexError::ParseError(format!("failed to parse nixosConfigurations names: {}", e))
    })?;

    if names.is_empty() {
        return Err(IndexError::NixBuildFailed(
            "no nixosConfigurations found in flake".to_string(),
        ));
    }

    if let Ok(out) = Command::new("hostname").output() {
        let hostname = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if names.contains(&hostname) {
            return Ok(hostname);
        }
    }

    // Fall back to the first available configuration.
    Ok(names.into_iter().next().unwrap())
}

/// Run `nix build` to produce the NixOS `options.json` derivation.
///
/// Returns the path of the output symlink (i.e. the result root, not the
/// `options.json` itself).  The caller is responsible for reading the file
/// at `<out_link>/share/doc/nixos/options.json` and removing the symlink
/// afterwards.
fn nix_build_options(flake_path: &Path, hostname: &str) -> Result<PathBuf, IndexError> {
    let flake_ref = flake_path.to_string_lossy();
    let attr = format!(
        "{}#nixosConfigurations.{}.config.system.build.manual.optionsJSON",
        flake_ref, hostname
    );

    // Use a process-unique path to avoid collisions between concurrent runs.
    let out_link = std::env::temp_dir()
        .join(format!("nixman-options-{}", std::process::id()));

    let output = Command::new("nix")
        .args(["build", &attr, "-o", &out_link.to_string_lossy()])
        .output()
        .map_err(IndexError::IoError)?;

    if !output.status.success() {
        let _ = fs::remove_file(&out_link);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(IndexError::NixBuildFailed(stderr));
    }

    Ok(out_link)
}

/// Parse a NixOS `options.json` file into a sorted list of [`OptionMeta`].
fn parse_options_json(path: &Path) -> Result<Vec<OptionMeta>, IndexError> {
    let content = fs::read_to_string(path)?;
    let raw: HashMap<String, RawOption> = serde_json::from_str(&content).map_err(|e| {
        IndexError::ParseError(format!("failed to parse options.json: {}", e))
    })?;
    raw_options_to_meta(raw)
}

/// Convert a `HashMap<String, RawOption>` into sorted `Vec<OptionMeta>`.
fn raw_options_to_meta(raw: HashMap<String, RawOption>) -> Result<Vec<OptionMeta>, IndexError> {
    let mut options: Vec<OptionMeta> = raw
        .into_iter()
        .map(|(path_str, raw)| OptionMeta {
            path: path_str,
            option_type: parse_option_type(&raw.type_str),
            default: value_to_string(&raw.default),
            description: clean_description(&value_to_string(&raw.description).unwrap_or_default()),
            declared_in: extract_declaration(&raw.declarations),
            example: value_to_string(&raw.example),
        })
        .collect();

    options.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(options)
}

/// Convert a `serde_json::Value` default/example field to a `String`.
///
/// Handles the `{ _type, text }` wrappers that NixOS uses for
/// `literalExpression` and `literalMD` values.
fn value_to_string(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::Null => None,
        v => {
            // Unwrap NixOS literal wrappers: { _type: "…", text: "…" }
            if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                Some(text.to_string())
            } else {
                match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Bool(b) => Some(b.to_string()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    other => Some(other.to_string()),
                }
            }
        }
    }
}

/// Extract the primary declaration path from the `declarations` array.
///
/// NixOS ≤ 22.11 stores bare strings; newer versions may store
/// `{ name, column, line }` objects.  Both are handled.
fn extract_declaration(declarations: &[serde_json::Value]) -> String {
    declarations
        .first()
        .and_then(|v| {
            // Bare string form.
            v.as_str()
                .map(str::to_string)
                // Object form: { name: "…", … }
                .or_else(|| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        })
        .unwrap_or_default()
}

/// Strip known DocBook/XML tags from NixOS option descriptions.
///
/// NixOS option descriptions are often written in DocBook (XML) or Markdown
/// and may contain tags like `<para>`, `<emphasis>`, etc.  This removes
/// only known tag names so that literal `<` or `>` characters (e.g., from
/// Nix expressions like `someOption < 5`) are preserved.
fn clean_description(s: &str) -> String {
    let known_tags: &[&str] = &[
        "para", "emphasis", "code", "literal", "option", "filename",
        "command", "replaceable", "programlisting", "note", "warning",
        "caution", "important", "tip", "simpara", "term", "varlistentry",
        "variablelist", "link", "xref", "ulink", "package", "envar",
        "varname", "function", "type", "classname", "methodname",
        "listitem", "itemizedlist", "orderedlist",
    ];

    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < len {
        if chars[i] == '<' {
            let start = i + 1;
            if start < len {
                let tag_content_start = if chars[start] == '/' { start + 1 } else { start };

                if tag_content_start < len && chars[tag_content_start].is_alphabetic() {
                    // Extract the tag name (up to whitespace, '>', or '/')
                    let mut tag_name_end = tag_content_start;
                    while tag_name_end < len
                        && !chars[tag_name_end].is_whitespace()
                        && chars[tag_name_end] != '>'
                        && chars[tag_name_end] != '/'
                    {
                        tag_name_end += 1;
                    }

                    let tag_name: String = chars[tag_content_start..tag_name_end].iter().collect();

                    if known_tags.contains(&tag_name.as_str()) {
                        let mut close = i;
                        while close < len && chars[close] != '>' {
                            close += 1;
                        }
                        if close < len {
                            i = close + 1;
                            continue;
                        }
                    }
                }
            }
            // Not a known tag — emit the '<' as a literal character
            out.push(chars[i]);
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out.trim().to_string()
}

/// Parse a NixOS type string into an [`OptionType`] variant.
///
/// Handles the common types produced by the NixOS module system.  Unknown
/// or complex types map to [`OptionType::Unspecified`].
pub(crate) fn parse_option_type(s: &str) -> OptionType {
    let s = s.trim();

    match s {
        "boolean" => return OptionType::Bool,
        "string" | "non-empty string" => return OptionType::String,
        "signed integer" | "integer" => return OptionType::Int,
        "floating point number" | "float" => return OptionType::Float,
        "path" => return OptionType::Path,
        "package" => return OptionType::Package,
        "submodule" => return OptionType::Submodule,
        "" | "unspecified" => return OptionType::Unspecified,
        _ => {}
    }

    // Catch all integer variants: "unsigned integer", "16 bit unsigned integer; …", etc.
    if s.contains("integer") || s.contains("unsigned") {
        return OptionType::Int;
    }

    if let Some(rest) = s.strip_prefix("list of ") {
        let inner = rest.trim_start_matches('(').trim_end_matches(')');
        return OptionType::ListOf(Box::new(parse_option_type(inner)));
    }

    if let Some(rest) = s.strip_prefix("attribute set of ") {
        let inner = rest.trim_start_matches('(').trim_end_matches(')');
        return OptionType::AttrsOf(Box::new(parse_option_type(inner)));
    }

    if let Some(rest) = s.strip_prefix("one of ") {
        return OptionType::Enum(parse_enum_variants(rest));
    }

    // "null or X", "null or boolean", etc. — fall through to Unspecified.
    OptionType::Unspecified
}

/// Extract quoted string variants from the suffix of a `"one of …"` type string.
///
/// Input: `r#""tcp", "udp""#` → `vec!["tcp", "udp"]`.
fn parse_enum_variants(s: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '"' {
            let mut variant = String::new();
            for c2 in chars.by_ref() {
                if c2 == '"' {
                    break;
                }
                variant.push(c2);
            }
            if !variant.is_empty() {
                variants.push(variant);
            }
        }
    }

    variants
}


/// Discover the `homeConfigurations` username for the flake at `hm_path`.
fn find_hm_username(flake_path: &Path) -> Result<String, IndexError> {
    let flake_ref = flake_path.to_string_lossy();
    let attr = format!("{}#homeConfigurations", flake_ref);

    let output = Command::new("nix")
        .args(["eval", &attr, "--apply", "builtins.attrNames", "--json"])
        .output()
        .map_err(IndexError::IoError)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(IndexError::NixBuildFailed(format!(
            "failed to list homeConfigurations: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let names: Vec<String> = serde_json::from_str(&stdout).map_err(|e| {
        IndexError::ParseError(format!("failed to parse homeConfigurations names: {}", e))
    })?;

    if names.is_empty() {
        return Err(IndexError::NixBuildFailed(
            "no homeConfigurations found in flake".to_string(),
        ));
    }

    if let Ok(out) = std::process::Command::new("whoami").output() {
        let username = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if names.contains(&username) {
            return Ok(username);
        }
    }

    // Fall back to the first available configuration.
    Ok(names.into_iter().next().unwrap())
}

/// Run `nix eval` to get the HM options JSON for `username` in the flake at `hm_path`.
fn nix_eval_hm_options(flake_path: &Path, username: &str) -> Result<HashMap<String, RawOption>, IndexError> {
    let flake_ref = flake_path.to_string_lossy();
    let attr = format!("{}#homeConfigurations.{}.options", flake_ref, username);

    let output = Command::new("nix")
        .args(["eval", &attr, "--json"])
        .output()
        .map_err(IndexError::IoError)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(IndexError::NixBuildFailed(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| {
        IndexError::ParseError(format!("failed to parse HM options: {}", e))
    })
}

/// Evaluate the full Home Manager option set for the flake at `hm_path` and
/// return a populated [`OptionIndex`].
///
/// # Progress
///
/// Same convention as [`build`]:
/// - `0.0` — immediately on entry
/// - `0.5` — after `nix eval` completes
/// - `1.0` — after parsing is complete
pub fn build_hm(hm_path: &Path, progress_tx: mpsc::Sender<f32>) -> Result<OptionIndex, IndexError> {
    let _ = progress_tx.send(0.0);

    let lock_path = hm_path.join("flake.lock");
    if !lock_path.exists() {
        return Err(IndexError::FlakeLockNotFound);
    }

    let flake_lock_hash = cache::hash_flake_lock(hm_path).map_err(|e| {
        IndexError::IoError(io::Error::new(io::ErrorKind::Other, e.to_string()))
    })?;

    let nixpkgs_rev = read_nixpkgs_rev(hm_path)?;

    let username = find_hm_username(hm_path)?;

    let raw_options = nix_eval_hm_options(hm_path, &username)?;
    let _ = progress_tx.send(0.5);

    let options = raw_options_to_meta(raw_options)?;
    let _ = progress_tx.send(1.0);

    Ok(OptionIndex {
        options,
        flake_lock_hash,
        built_at: Utc::now(),
        nixpkgs_rev,
    })
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_boolean() {
        assert!(matches!(parse_option_type("boolean"), OptionType::Bool));
    }

    #[test]
    fn type_string() {
        assert!(matches!(parse_option_type("string"), OptionType::String));
        assert!(matches!(
            parse_option_type("non-empty string"),
            OptionType::String
        ));
    }

    #[test]
    fn type_signed_integer() {
        assert!(matches!(parse_option_type("signed integer"), OptionType::Int));
    }

    #[test]
    fn type_sized_unsigned_integer() {
        assert!(matches!(
            parse_option_type("16 bit unsigned integer; between 0 and 65535 (both inclusive)"),
            OptionType::Int
        ));
    }

    #[test]
    fn type_float() {
        assert!(matches!(
            parse_option_type("floating point number"),
            OptionType::Float
        ));
    }

    #[test]
    fn type_path() {
        assert!(matches!(parse_option_type("path"), OptionType::Path));
    }

    #[test]
    fn type_package() {
        assert!(matches!(parse_option_type("package"), OptionType::Package));
    }

    #[test]
    fn type_submodule() {
        assert!(matches!(
            parse_option_type("submodule"),
            OptionType::Submodule
        ));
    }

    #[test]
    fn type_list_of_string() {
        match parse_option_type("list of string") {
            OptionType::ListOf(inner) => assert!(matches!(*inner, OptionType::String)),
            other => panic!("expected ListOf, got {:?}", other),
        }
    }

    #[test]
    fn type_list_of_package_parenthesised() {
        match parse_option_type("list of (package)") {
            OptionType::ListOf(inner) => assert!(matches!(*inner, OptionType::Package)),
            other => panic!("expected ListOf, got {:?}", other),
        }
    }

    #[test]
    fn type_attribute_set_of_string() {
        match parse_option_type("attribute set of string") {
            OptionType::AttrsOf(inner) => assert!(matches!(*inner, OptionType::String)),
            other => panic!("expected AttrsOf, got {:?}", other),
        }
    }

    #[test]
    fn type_enum() {
        match parse_option_type(r#"one of "tcp", "udp""#) {
            OptionType::Enum(variants) => assert_eq!(variants, vec!["tcp", "udp"]),
            other => panic!("expected Enum, got {:?}", other),
        }
    }

    #[test]
    fn type_unknown_is_unspecified() {
        assert!(matches!(
            parse_option_type("some weird composite type"),
            OptionType::Unspecified
        ));
    }

    #[test]
    fn type_empty_is_unspecified() {
        assert!(matches!(parse_option_type(""), OptionType::Unspecified));
    }

    #[test]
    fn value_null_returns_none() {
        assert_eq!(value_to_string(&serde_json::Value::Null), None);
    }

    #[test]
    fn value_literal_expression_unwrapped() {
        let v = serde_json::json!({"_type": "literalExpression", "text": "false"});
        assert_eq!(value_to_string(&v), Some("false".to_string()));
    }

    #[test]
    fn value_md_doc_unwrapped() {
        let v = serde_json::json!({"_type": "mdDoc", "text": "some markdown"});
        assert_eq!(value_to_string(&v), Some("some markdown".to_string()));
    }

    #[test]
    fn value_plain_bool() {
        assert_eq!(
            value_to_string(&serde_json::json!(true)),
            Some("true".to_string())
        );
    }

    #[test]
    fn value_plain_string() {
        assert_eq!(
            value_to_string(&serde_json::json!("hello")),
            Some("hello".to_string())
        );
    }

    #[test]
    fn strips_xml_tags() {
        let raw = "<para>Enable <emphasis>nginx</emphasis>.</para>";
        assert_eq!(clean_description(raw), "Enable nginx.");
    }

    #[test]
    fn plain_text_unchanged() {
        let raw = "Whether to enable nginx.";
        assert_eq!(clean_description(raw), "Whether to enable nginx.");
    }

    #[test]
    fn preserves_literal_angle_operators() {
        let raw = "someOption < 5 and someOption > 10";
        assert_eq!(
            clean_description(raw),
            "someOption < 5 and someOption > 10"
        );
    }

    #[test]
    fn preserves_nix_expression_gt_operator() {
        let raw = "This only works with NixOS >= 24.11.";
        assert_eq!(clean_description(raw), "This only works with NixOS >= 24.11.");
    }

    #[test]
    fn strips_self_closing_tags() {
        let raw = "Some text<para/>more text";
        assert_eq!(clean_description(raw), "Some textmore text");
    }

    #[test]
    fn strips_tags_with_attributes() {
        let raw = r#"<emphasis role="bold">Important</emphasis>"#;
        assert_eq!(clean_description(raw), "Important");
    }

    #[test]
    fn handles_mixed_known_tags_and_literal_operators() {
        let raw = "<para>Set to <literal>1</literal> if value > 10</para>";
        assert_eq!(clean_description(raw), "Set to 1 if value > 10");
    }

    #[test]
    fn strips_unknown_tag_content() {
        // Unknown tags like <foobar> are NOT stripped (they pass through as literal)
        let raw = "some <custom> text";
        assert_eq!(clean_description(raw), "some <custom> text");
    }

    #[test]
    fn enum_variants_parsed() {
        let variants = parse_enum_variants(r#""tcp", "udp", "sctp""#);
        assert_eq!(variants, vec!["tcp", "udp", "sctp"]);
    }

    #[test]
    fn declaration_bare_string() {
        let decls = vec![serde_json::json!("nixos/modules/foo.nix")];
        assert_eq!(extract_declaration(&decls), "nixos/modules/foo.nix");
    }

    #[test]
    fn declaration_object_form() {
        let decls = vec![serde_json::json!({"name": "nixos/modules/bar.nix", "line": 42, "column": 1})];
        assert_eq!(extract_declaration(&decls), "nixos/modules/bar.nix");
    }

    #[test]
    fn declaration_empty_returns_default() {
        assert_eq!(extract_declaration(&[]), "");
    }
}
