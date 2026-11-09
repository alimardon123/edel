//! The bill of materials of an image (roadmap M8.12, ADR-009): every
//! package apk installed, with its version, licence and origin, as a
//! CycloneDX 1.6 JSON document written beside the image
//! (`out/<image>.cdx.json`). Read from apk's own record of the root,
//! `lib/apk/db/installed`; written by hand, as JSON is all it needs
//! (no new dependency). Nothing of it runs on machines. Only the image
//! builder (`image.rs`) uses it: Alpine's own paths live only in the
//! image builder, `edel::places` and `edel::base` (ADR-010).

/// apk's record of the installed packages, relative to the root (M8.12).
pub const INSTALLED_DB: &str = "lib/apk/db/installed";

/// One installed package, as apk records it. A missing field is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The package name (apk's `P` line).
    pub name: String,
    /// The version, such as `1.2.5-r9` (apk's `V` line).
    pub version: String,
    /// The architecture (apk's `A` line).
    pub arch: String,
    /// The licence as apk writes it (apk's `L` line), not always SPDX. The
    /// document writes it as an SPDX id or expression when every id in it is
    /// SPDX's (`spdx.txt`), else as apk's words.
    pub licence: String,
    /// The origin package the name was built from (apk's `o` line).
    pub origin: String,
}

/// Reads apk's installed database: records separated by blank lines, each
/// line `X:value`. Keeps `P`, `V`, `A`, `L` and `o` and ignores the rest.
/// A record without a name or version is skipped, and a missing origin is
/// the name. The result is sorted by name, then version.
pub fn parse_installed(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for record in text.split("\n\n") {
        let (mut name, mut version, mut arch, mut licence, mut origin) =
            (None, None, None, None, None);
        for line in record.lines() {
            let [key, b':', ..] = line.as_bytes() else {
                continue;
            };
            // Byte 1 is the ASCII colon, so byte 2 starts a character.
            let value = &line[2..];
            match key {
                b'P' => name = Some(value),
                b'V' => version = Some(value),
                b'A' => arch = Some(value),
                b'L' => licence = Some(value),
                b'o' => origin = Some(value),
                _ => {}
            }
        }
        let (Some(name), Some(version)) = (name, version) else {
            continue;
        };
        if name.is_empty() || version.is_empty() {
            continue;
        }
        packages.push(Package {
            name: name.to_string(),
            version: version.to_string(),
            arch: arch.unwrap_or("").to_string(),
            licence: licence.unwrap_or("").to_string(),
            origin: origin.unwrap_or(name).to_string(),
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));
    packages
}

/// The package list a release carries beside each image and add-on
/// (`out/NAME.packages`): one `name-version` a line, sorted.
pub fn list(packages: &[Package]) -> String {
    let mut lines: Vec<String> = packages
        .iter()
        .map(|p| format!("{}-{}\n", p.name, p.version))
        .collect();
    lines.sort_unstable();
    lines.concat()
}

/// The packages of `after` that `before` lacks at the same version: what
/// an add-on, built from a copy of the image's root, installs itself
/// (M8.42).
pub fn added(before: &[Package], after: &[Package]) -> Vec<Package> {
    after
        .iter()
        .filter(|p| {
            !before
                .iter()
                .any(|b| b.name == p.name && b.version == p.version)
        })
        .cloned()
        .collect()
}

/// SPDX's licence and exception ids, one a line, with `#` comments.
const SPDX_IDS: &str = include_str!("spdx.txt");

/// Whether `id` is one of the ids in `spdx.txt`, exactly as written.
fn in_spdx_list(id: &str) -> bool {
    SPDX_IDS
        .lines()
        .filter(|line| !line.starts_with('#'))
        .any(|line| line == id)
}

/// Whether `id` is an SPDX id, or a `LicenseRef-` id of our own: the
/// prefix, then one or more ASCII letters, digits, `.` or `-`.
fn is_spdx_id(id: &str) -> bool {
    if in_spdx_list(id) {
        return true;
    }
    id.strip_prefix("LicenseRef-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    })
}

/// Whether a token is one of the operators or brackets of an SPDX expression.
fn is_operator(token: &str) -> bool {
    matches!(token, "AND" | "OR" | "WITH" | "(" | ")")
}

/// apk's licence text as an SPDX expression, or `None` when its words are
/// not one (`GPL` alone, a misspelt id, a broken bracket). apk lists several
/// licences side by side, all of which apply, so `MIT ISC` becomes
/// `MIT AND ISC`. `custom` and `Public Domain` become `LicenseRef-` ids.
pub fn spdx_expression(licence: &str) -> Option<String> {
    let text = licence.replace("Public Domain", "Public-Domain");
    let spaced = text.replace('(', " ( ").replace(')', " ) ");
    let mut tokens: Vec<String> = spaced
        .split_whitespace()
        .map(|t| {
            if t == "custom" || t.starts_with("custom:") {
                "LicenseRef-custom".to_string()
            } else if t == "Public-Domain" {
                "LicenseRef-Public-Domain".to_string()
            } else {
                t.to_string()
            }
        })
        .collect();
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() >= 2 && !tokens.iter().any(|t| is_operator(t)) {
        let mut ids: Vec<String> = Vec::new();
        for t in tokens {
            if !ids.contains(&t) {
                ids.push(t);
            }
        }
        tokens = ids.join(" AND ").split(' ').map(String::from).collect();
    }

    // Each token is an operand (an id, after `(`, `AND`, `OR` or at the
    // start) or an operator (after an operand or `)`).
    // `WITH` comes only after an id, and names one of SPDX's own exceptions.
    let mut depth = 0usize;
    let mut want_operand = true;
    let mut after_id = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if want_operand {
            match t {
                "(" => depth += 1,
                _ if is_spdx_id(t) => {
                    want_operand = false;
                    after_id = true;
                }
                _ => return None,
            }
        } else {
            match t {
                "AND" | "OR" => want_operand = true,
                ")" => depth = depth.checked_sub(1)?,
                "WITH" if after_id => {
                    let exception = tokens.get(i + 1)?;
                    if !in_spdx_list(exception) {
                        return None;
                    }
                    i += 1;
                }
                _ => return None,
            }
            after_id = false;
        }
        i += 1;
    }
    if want_operand || depth != 0 {
        return None;
    }

    let mut out = String::new();
    for (i, t) in tokens.iter().enumerate() {
        if i > 0 && tokens[i - 1] != "(" && t != ")" {
            out.push(' ');
        }
        out.push_str(t);
    }
    Some(out)
}

/// The `licenses` entry of a component: apk's licence as an SPDX id when it
/// is one id, as an SPDX expression when it is several, else apk's words
/// as a name (M8.12).
fn licences(licence: &str) -> String {
    match spdx_expression(licence) {
        Some(id) if in_spdx_list(&id) => format!(
            "      \"licenses\": [\n        {{\n          \"license\": {{\n            \"id\": {}\n          }}\n        }}\n      ]",
            json(&id)
        ),
        Some(expression) => format!(
            "      \"licenses\": [\n        {{\n          \"expression\": {}\n        }}\n      ]",
            json(&expression)
        ),
        None => format!(
            "      \"licenses\": [\n        {{\n          \"license\": {{\n            \"name\": {}\n          }}\n        }}\n      ]",
            json(licence)
        ),
    }
}

/// Writes the CycloneDX 1.6 document of an image. `image` is the image's
/// stem, `version` the build's version (empty leaves it out), `distro` the
/// purl's distro qualifier (`alpine-3.24`). Nothing in it changes between
/// two builds of one commit: there is no serial number and no timestamp.
pub fn cyclonedx(image: &str, version: &str, distro: &str, packages: &[Package]) -> String {
    let mut meta = vec![
        "      \"type\": \"operating-system\"".to_string(),
        format!("      \"bom-ref\": {}", json(image)),
        format!("      \"name\": {}", json(image)),
    ];
    if !version.is_empty() {
        meta.push(format!("      \"version\": {}", json(version)));
    }
    let components = if packages.is_empty() {
        "[]".to_string()
    } else {
        let items: Vec<String> = packages.iter().map(|p| component(p, distro)).collect();
        format!("[\n{}\n  ]", items.join(",\n"))
    };
    format!(
        "{{\n  \"bomFormat\": \"CycloneDX\",\n  \"specVersion\": \"1.6\",\n  \"version\": 1,\n  \"metadata\": {{\n    \"component\": {{\n{}\n    }}\n  }},\n  \"components\": {components}\n}}\n",
        meta.join(",\n")
    )
}

/// One `components` entry, indented as `cyclonedx` writes it.
fn component(p: &Package, distro: &str) -> String {
    let purl = purl(p, distro);
    let mut fields = vec![
        "      \"type\": \"library\"".to_string(),
        format!("      \"bom-ref\": {}", json(&purl)),
        format!("      \"name\": {}", json(&p.name)),
        format!("      \"version\": {}", json(&p.version)),
        format!("      \"purl\": {}", json(&purl)),
    ];
    if !p.licence.is_empty() {
        fields.push(licences(&p.licence));
    }
    fields.push(format!(
        "      \"properties\": [\n        {{\n          \"name\": \"apk:origin\",\n          \"value\": {}\n        }}\n      ]",
        json(&p.origin)
    ));
    format!("    {{\n{}\n    }}", fields.join(",\n"))
}

/// The package URL of a package, `arch` left out when it is empty.
fn purl(p: &Package, distro: &str) -> String {
    let arch = if p.arch.is_empty() {
        String::new()
    } else {
        format!("arch={}&", purl_part(&p.arch))
    };
    format!(
        "pkg:apk/alpine/{}@{}?{arch}distro={}",
        purl_part(&p.name),
        purl_part(&p.version),
        purl_part(distro)
    )
}

/// A string as a JSON string literal, quoted and escaped.
fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Percent-encodes a purl part: everything but ASCII letters, digits, `.`,
/// `-`, `_` and `~` becomes `%XX` for each byte of its UTF-8.
fn purl_part(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, version: &str) -> Package {
        Package {
            name: name.to_string(),
            version: version.to_string(),
            arch: "x86_64".to_string(),
            licence: String::new(),
            origin: name.to_string(),
        }
    }

    #[test]
    fn an_addon_records_only_the_packages_it_adds() {
        let image = [package("musl", "1.2.6-r0"), package("zlib", "1.3.1-r2")];
        let addon = [
            package("gtk4.0", "4.22.0-r0"),
            package("musl", "1.2.6-r0"),
            package("zlib", "1.3.1-r3"),
        ];
        let own = added(&image, &addon);
        assert_eq!(list(&own), "gtk4.0-4.22.0-r0\nzlib-1.3.1-r3\n");
    }

    #[test]
    fn reads_each_record_of_the_installed_database() {
        let text = "\
P:zlib
V:1.3.1-r2
A:x86_64
L:Zlib
o:zlib
D:so:libc.musl-x86_64.so.1
F:lib
R:libz.so.1.3.1
a:0:0:755

P:busybox
V:1.37.0-r30
A:x86_64
L:GPL-2.0-only

V:9.9-r0
A:x86_64
L:MIT
";
        let packages = parse_installed(text);
        assert_eq!(
            packages,
            vec![
                Package {
                    name: "busybox".into(),
                    version: "1.37.0-r30".into(),
                    arch: "x86_64".into(),
                    licence: "GPL-2.0-only".into(),
                    origin: "busybox".into(),
                },
                Package {
                    name: "zlib".into(),
                    version: "1.3.1-r2".into(),
                    arch: "x86_64".into(),
                    licence: "Zlib".into(),
                    origin: "zlib".into(),
                },
            ]
        );
    }

    #[test]
    fn writes_a_cyclonedx_document() {
        let package = Package {
            name: "musl".into(),
            version: "1.2.5-r9".into(),
            arch: "x86_64".into(),
            licence: "MIT".into(),
            origin: "musl".into(),
        };
        let text = cyclonedx("edel-vm-x86_64", "2026.10.5", "alpine-3.24", &[package]);
        let expected = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.6",
  "version": 1,
  "metadata": {
    "component": {
      "type": "operating-system",
      "bom-ref": "edel-vm-x86_64",
      "name": "edel-vm-x86_64",
      "version": "2026.10.5"
    }
  },
  "components": [
    {
      "type": "library",
      "bom-ref": "pkg:apk/alpine/musl@1.2.5-r9?arch=x86_64&distro=alpine-3.24",
      "name": "musl",
      "version": "1.2.5-r9",
      "purl": "pkg:apk/alpine/musl@1.2.5-r9?arch=x86_64&distro=alpine-3.24",
      "licenses": [
        {
          "license": {
            "id": "MIT"
          }
        }
      ],
      "properties": [
        {
          "name": "apk:origin",
          "value": "musl"
        }
      ]
    }
  ]
}
"#;
        assert_eq!(text, expected);
    }

    #[test]
    fn encodes_names_for_the_purl_and_escapes_json() {
        let package = Package {
            name: "libstdc++".into(),
            version: "14.2.0-r6".into(),
            arch: "x86_64".into(),
            licence: "GPL-3.0-or-later \"with\" exception\\".into(),
            origin: "gcc".into(),
        };
        let text = cyclonedx("edel-vm-x86_64", "", "alpine-3.24", &[package]);
        assert!(
            text.contains("pkg:apk/alpine/libstdc%2B%2B@14.2.0-r6?arch=x86_64&distro=alpine-3.24")
        );
        assert!(text.contains(r#""GPL-3.0-or-later \"with\" exception\\""#));
        assert_eq!(json("a\u{1}"), r#""a\u0001""#);
    }

    #[test]
    fn spdx_expression_follows_apks_licence_lines() {
        let cases: [(&str, Option<&str>); 16] = [
            ("MIT", Some("MIT")),
            (
                "GPL-2.0-or-later AND LGPL-2.1-or-later",
                Some("GPL-2.0-or-later AND LGPL-2.1-or-later"),
            ),
            (
                "GPL-2.0-or-later LGPL-2.1-or-later MIT",
                Some("GPL-2.0-or-later AND LGPL-2.1-or-later AND MIT"),
            ),
            (
                "GPL-3.0-or-later AND ( GPL-2.0-or-later OR LGPL-3.0-or-later )",
                Some("GPL-3.0-or-later AND (GPL-2.0-or-later OR LGPL-3.0-or-later)"),
            ),
            (
                "GPL-2.0-or-later WITH cryptsetup-OpenSSL-exception",
                Some("GPL-2.0-or-later WITH cryptsetup-OpenSSL-exception"),
            ),
            ("custom", Some("LicenseRef-custom")),
            ("MIT custom", Some("MIT AND LicenseRef-custom")),
            (
                "BSD-3-Clause AND BSD-2-Clause AND ISC AND Beerware AND Public Domain",
                Some(
                    "BSD-3-Clause AND BSD-2-Clause AND ISC AND Beerware AND LicenseRef-Public-Domain",
                ),
            ),
            ("GPL", None),
            (
                "GPL-2.0-or-later GPL-2.0-or-later-with-sane-exception Public-Domain",
                None,
            ),
            ("MIT AND", None),
            ("(MIT", None),
            ("MIT WITH", None),
            ("(MIT OR ISC) WITH LLVM-exception", None),
            ("MIT WITH LicenseRef-custom", None),
            ("", None),
        ];
        for (apk, expected) in cases {
            assert_eq!(spdx_expression(apk).as_deref(), expected, "for {apk:?}");
        }
    }

    #[test]
    fn writes_an_expression_for_several_licences() {
        let package = Package {
            name: "libgcc".into(),
            version: "14.2.0-r6".into(),
            arch: "x86_64".into(),
            licence: "GPL-2.0-or-later LGPL-2.1-or-later".into(),
            origin: "gcc".into(),
        };
        let text = cyclonedx("edel-vm-x86_64", "", "alpine-3.24", &[package]);
        assert!(text.contains(r#""expression": "GPL-2.0-or-later AND LGPL-2.1-or-later""#));
    }

    #[test]
    fn leaves_out_what_is_empty() {
        let package = Package {
            name: "busybox".into(),
            version: "1.37.0-r30".into(),
            arch: String::new(),
            licence: String::new(),
            origin: "busybox".into(),
        };
        let text = cyclonedx("edel-vm-x86_64", "", "alpine-3.24", &[package]);
        assert!(!text.contains("\"licenses\""));
        assert!(!text.contains("arch="));
        assert!(!text.contains("\"version\": \"\""));
        assert!(text.contains("pkg:apk/alpine/busybox@1.37.0-r30?distro=alpine-3.24"));
        let empty = cyclonedx("edel-vm-x86_64", "", "alpine-3.24", &[]);
        assert!(empty.contains("\"components\": []"));
    }
}
