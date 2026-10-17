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
    /// The licence as apk writes it (apk's `L` line), not always SPDX.
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
        fields.push(format!(
            "      \"licenses\": [\n        {{\n          \"license\": {{\n            \"name\": {}\n          }}\n        }}\n      ]",
            json(&p.licence)
        ));
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
            "name": "MIT"
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
