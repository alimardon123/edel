//! Signed releases (roadmap M1.6). `release.toml` lists the images of one
//! release with their sha256 and size; `release.toml.sig` is an ed25519
//! signature over its exact bytes. Every image carries two public keys in
//! `/usr/share/edel/keys/`, so a key can be replaced without stranding a
//! machine. `edel update install` refuses a manifest no key signed, an
//! image whose sha256 differs and a version that is not newer.
//!
//! The manifest is the one file an old slot reads from the future, so its
//! reader ignores fields it does not know (the signature still covers them)
//! and refuses only a format it does not know (ADR-008, docs/FORMATS.md).

use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The only manifest format this edel reads.
pub const FORMAT: i64 = 1;
/// Where images keep the public keys that may sign their updates.
pub const KEYS_DIR: &str = "/usr/share/edel/keys";

/// `release.toml`. No `deny_unknown_fields`: a newer release may add keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: i64,
    pub version: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub images: Vec<ImageEntry>,
}

/// One image of a release. `sha256` and `size` are those of the image as
/// it lands in the slot, after a `.gz` file is decompressed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEntry {
    /// The image's name, as in `EDEL_IMAGE` (`edel-vm-x86_64`).
    pub name: String,
    /// The file, relative to `release.toml`.
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(text: &str) -> Result<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("not a hex string: {text:?}");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| Ok(u8::from_str_radix(&text[i..i + 2], 16)?))
        .collect()
}

fn fixed<const N: usize>(bytes: Vec<u8>, what: &str) -> Result<[u8; N]> {
    let len = bytes.len();
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{what} has {len} bytes, not {N}"))
}

/// A public key file: 32 bytes as hex.
pub fn read_public_key(path: &Path) -> Result<VerifyingKey> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let bytes = fixed::<32>(from_hex(&text)?, "the public key")?;
    VerifyingKey::from_bytes(&bytes)
        .with_context(|| format!("{} is not an ed25519 public key", path.display()))
}

/// Every `*.pub` key in `dir`.
fn load_keys(dir: &Path) -> Result<Vec<VerifyingKey>> {
    let mut keys = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "pub") {
                keys.push(read_public_key(&path)?);
            }
        }
    }
    if keys.is_empty() {
        bail!(
            "no public keys in {}; this image cannot check updates",
            dir.display()
        );
    }
    Ok(keys)
}

/// Checks `sig_hex` over `bytes` against every key; one must match.
fn verify_bytes(bytes: &[u8], sig_hex: &str, keys: &[VerifyingKey]) -> Result<()> {
    let signature = from_hex(sig_hex)
        .and_then(|b| fixed::<64>(b, "the signature"))
        .map(|b| Signature::from_bytes(&b));
    if let Ok(signature) = signature {
        if keys
            .iter()
            .any(|k| k.verify_strict(bytes, &signature).is_ok())
        {
            return Ok(());
        }
    }
    bail!("refused: signature: no trusted key signed this release")
}

/// Parses a manifest, refusing only a format this edel does not know.
pub fn parse_manifest(text: &str) -> Result<Manifest> {
    let table: toml::Table = toml::from_str(text).context("release.toml is not valid TOML")?;
    let format = table
        .get("format")
        .and_then(|v| v.as_integer())
        .context("release.toml has no format")?;
    if format != FORMAT {
        bail!(
            "refused: release.toml is format {format}; this edel reads format {FORMAT}, so install a release in between first"
        );
    }
    Ok(toml::from_str(text)?)
}

/// Compares dotted versions number by number (`2026.10.2` > `2026.9.9`);
/// a missing part counts as 0 and a part that is not a number compares as
/// text.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<String> { v.split('.').map(str::to_string).collect() };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).map_or("0", String::as_str),
            b.get(i).map_or("0", String::as_str),
        );
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// The entry for `image` in `manifest`, refused when the version is not
/// newer than `running` (unless `allow_downgrade`).
fn pick_image<'a>(
    manifest: &'a Manifest,
    image: &str,
    running: &str,
    allow_downgrade: bool,
) -> Result<&'a ImageEntry> {
    if !allow_downgrade && compare_versions(&manifest.version, running) != Ordering::Greater {
        bail!(
            "refused: version {} is not newer than the running {running}; pass --allow-downgrade to install it anyway",
            manifest.version
        );
    }
    manifest
        .images
        .iter()
        .find(|e| e.name == image)
        .with_context(|| {
            format!(
                "refused: release {} has no image {image:?}",
                manifest.version
            )
        })
}

fn is_url(location: &str) -> bool {
    location.starts_with("http://") || location.starts_with("https://")
}

fn local_path(location: &str) -> &str {
    location.strip_prefix("file://").unwrap_or(location)
}

/// The HTTP client: a mirror that does not answer fails the download
/// instead of hanging it (the image body itself may take as long as it
/// takes).
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// Reads a small file (a manifest or a signature) from a path or a URL.
fn fetch(location: &str) -> Result<Vec<u8>> {
    if is_url(location) {
        let mut response = agent()
            .get(location)
            .call()
            .with_context(|| format!("downloading {location}"))?;
        Ok(response.body_mut().read_to_vec()?)
    } else {
        let path = local_path(location);
        fs::read(path).with_context(|| format!("reading {path}"))
    }
}

/// Opens an image for streaming, decompressing a `.gz` file on the way.
fn open_image(location: &str) -> Result<Box<dyn Read>> {
    let raw: Box<dyn Read> = if is_url(location) {
        let response = agent()
            .get(location)
            .call()
            .with_context(|| format!("downloading {location}"))?;
        Box::new(response.into_body().into_reader())
    } else {
        let path = local_path(location);
        Box::new(File::open(path).with_context(|| format!("reading {path}"))?)
    };
    Ok(if location.ends_with(".gz") {
        Box::new(GzDecoder::new(raw))
    } else {
        raw
    })
}

/// `file` in the same directory as the manifest at `location`.
/// Where an image named in a manifest at `location` is: `file` itself when
/// it is a URL (a channel manifest pointing at a release's assets, M3.4),
/// otherwise beside the manifest.
fn beside(location: &str, file: &str) -> String {
    if is_url(file) {
        return file.to_string();
    }
    match location.rfind('/') {
        Some(i) => format!("{}/{file}", &location[..i]),
        None => file.to_string(),
    }
}

fn sha256_reader(reader: &mut dyn Read) -> Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((to_hex(&hasher.finalize()), size))
}

/// The manifest at `location` (a path or an http(s) URL), after its
/// signature is checked against the keys this image carries.
fn verified_manifest(location: &str) -> Result<Manifest> {
    let bytes = fetch(location)?;
    let sig = fetch(&format!("{location}.sig"))
        .context("refused: signature: cannot read the .sig file beside release.toml")?;
    verify_bytes(
        &bytes,
        &String::from_utf8_lossy(&sig),
        &load_keys(Path::new(KEYS_DIR))?,
    )?;
    parse_manifest(&String::from_utf8_lossy(&bytes))
}

/// A checked release, ready to stream into a slot. `sha256` and `size`
/// describe the uncompressed image, which is what lands on the disk.
pub struct Checked {
    pub sha256: Vec<u8>,
    pub size: u64,
    pub reader: Box<dyn Read>,
}

/// Checks the release at `location` with this image's keys and opens the
/// image for this machine.
pub fn open_checked(location: &str, allow_downgrade: bool) -> Result<Checked> {
    let manifest = verified_manifest(location)?;
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE")
        .context("this system's /usr/lib/os-release has no EDEL_IMAGE")?;
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let entry = pick_image(&manifest, &image, &running, allow_downgrade)?;
    let sha256 = from_hex(&entry.sha256).context("release.toml has a bad sha256")?;
    println!("release {}: signature checked", manifest.version);
    Ok(Checked {
        sha256,
        size: entry.size,
        reader: open_image(&beside(location, &entry.file))?,
    })
}

/// `edel update check`: the running version and the one at `location`.
pub fn check(location: &str) -> Result<()> {
    let manifest = verified_manifest(location)?;
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let newer = compare_versions(&manifest.version, &running) == Ordering::Greater;
    println!("running: {running}");
    println!(
        "available: {} ({})",
        manifest.version,
        if newer { "newer" } else { "not newer" }
    );
    Ok(())
}

/// The value of `key` in an os-release text, without quotes.
pub fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// `edel release keygen`: a new key pair, `NAME.key` (secret, mode 0600)
/// and `NAME.pub`, from the kernel's random numbers.
pub fn keygen(dir: &Path, name: &str) -> Result<()> {
    let mut seed = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut seed)?;
    let key = SigningKey::from_bytes(&seed);
    fs::create_dir_all(dir)?;
    let secret = dir.join(format!("{name}.key"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&secret)
        .with_context(|| format!("creating {}", secret.display()))?;
    std::io::Write::write_all(&mut file, format!("{}\n", to_hex(&seed)).as_bytes())?;
    let public = dir.join(format!("{name}.pub"));
    fs::write(
        &public,
        format!("{}\n", to_hex(key.verifying_key().as_bytes())),
    )?;
    println!("{} and {}", secret.display(), public.display());
    Ok(())
}

/// `edel release make`: `release.toml` beside the images it lists. With
/// `base_url` the manifest names each image by its URL there instead, so
/// it can be served from somewhere else (a channel, M3.4), and goes into
/// `out_dir` when given.
pub fn make(
    version: &str,
    channel: &str,
    base_url: Option<&str>,
    out_dir: Option<&Path>,
    images: &[PathBuf],
) -> Result<PathBuf> {
    if let Some(url) = base_url.filter(|u| !is_url(u)) {
        bail!("--base-url {url:?} is not an http(s) URL");
    }
    let Some(dir) = images.first().and_then(|i| i.parent()) else {
        bail!("name at least one image");
    };
    let mut entries = Vec::new();
    for image in images {
        if image.parent() != Some(dir) {
            bail!("all images must be in one directory, beside release.toml");
        }
        let file = image
            .file_name()
            .context("an image needs a file name")?
            .to_string_lossy()
            .into_owned();
        let name = file.split('.').next().unwrap_or_default().to_string();
        let location = image.to_str().context("an image path must be UTF-8")?;
        let (sha256, size) = sha256_reader(&mut open_image(location)?)?;
        let file = match base_url {
            Some(url) => format!("{}/{file}", url.trim_end_matches('/')),
            None => file,
        };
        entries.push(ImageEntry {
            name,
            file,
            sha256,
            size,
        });
    }
    let date = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let manifest = Manifest {
        format: FORMAT,
        version: version.to_string(),
        channel: channel.to_string(),
        date,
        images: entries,
    };
    let out = out_dir.unwrap_or(dir);
    fs::create_dir_all(out)?;
    let path = out.join("release.toml");
    fs::write(&path, toml::to_string(&manifest)?)?;
    println!("{}", path.display());
    Ok(path)
}

/// `edel release sign`: writes `FILE.sig`.
pub fn sign(key_path: &Path, file: &Path) -> Result<()> {
    let text =
        fs::read_to_string(key_path).with_context(|| format!("reading {}", key_path.display()))?;
    let key = SigningKey::from_bytes(&fixed::<32>(from_hex(&text)?, "the secret key")?);
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let sig = key.sign(&bytes);
    let sig_path = PathBuf::from(format!("{}.sig", file.display()));
    fs::write(&sig_path, format!("{}\n", to_hex(&sig.to_bytes())))?;
    println!("{}", sig_path.display());
    Ok(())
}

/// `edel release verify`: checks `FILE.sig` against the keys in `keys`.
pub fn verify(keys: &Path, file: &Path) -> Result<()> {
    let bytes = fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let sig = fs::read_to_string(format!("{}.sig", file.display()))?;
    verify_bytes(&bytes, &sig, &load_keys(keys)?)?;
    println!("{}: signature good", file.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_images_beside_the_manifest_or_at_their_url() {
        assert_eq!(
            beside(
                "https://example.org/r/preview/release.toml",
                "edel-vm-x86_64.ext4.gz"
            ),
            "https://example.org/r/preview/edel-vm-x86_64.ext4.gz"
        );
        assert_eq!(
            beside(
                "https://example.org/channels/stable/release.toml",
                "https://example.org/r/v1/edel-vm-x86_64.ext4.gz"
            ),
            "https://example.org/r/v1/edel-vm-x86_64.ext4.gz"
        );
        assert_eq!(beside("/srv/release.toml", "a.ext4"), "/srv/a.ext4");
    }

    const MANIFEST: &str = "format = 1\nversion = \"2026.10.2\"\nchannel = \"stable\"\n\n[[images]]\nname = \"edel-vm-x86_64\"\nfile = \"edel-vm-x86_64.ext4\"\nsha256 = \"ab\"\nsize = 2\n";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn sig(key: &SigningKey, text: &str) -> String {
        to_hex(&key.sign(text.as_bytes()).to_bytes())
    }

    #[test]
    fn signs_then_verifies_with_either_key() {
        let keys = [key(1).verifying_key(), key(2).verifying_key()];
        assert!(verify_bytes(MANIFEST.as_bytes(), &sig(&key(2), MANIFEST), &keys).is_ok());
    }

    #[test]
    fn refuses_a_tampered_manifest_and_a_wrong_key() {
        let keys = [key(1).verifying_key()];
        let good = sig(&key(1), MANIFEST);
        let tampered = MANIFEST.replace("stable", "stablf");
        let err = verify_bytes(tampered.as_bytes(), &good, &keys).unwrap_err();
        assert!(err.to_string().contains("refused: signature"));
        assert!(verify_bytes(MANIFEST.as_bytes(), &sig(&key(9), MANIFEST), &keys).is_err());
        assert!(verify_bytes(MANIFEST.as_bytes(), "not hex", &keys).is_err());
    }

    #[test]
    fn reads_unknown_fields_and_refuses_an_unknown_format() {
        let newer = format!("{MANIFEST}delta = \"later\"\n[future]\nx = 1\n");
        let keys = [key(1).verifying_key()];
        assert!(verify_bytes(newer.as_bytes(), &sig(&key(1), &newer), &keys).is_ok());
        let manifest = parse_manifest(&newer).unwrap();
        assert_eq!(manifest.images[0].name, "edel-vm-x86_64");
        let err = parse_manifest(&MANIFEST.replace("format = 1", "format = 2")).unwrap_err();
        assert!(err.to_string().contains("format 2"));
    }

    #[test]
    fn refuses_a_version_that_is_not_newer() {
        let manifest = parse_manifest(MANIFEST).unwrap();
        assert!(pick_image(&manifest, "edel-vm-x86_64", "2026.10.1", false).is_ok());
        let err = pick_image(&manifest, "edel-vm-x86_64", "2026.10.2", false).unwrap_err();
        assert!(err.to_string().contains("not newer"));
        assert!(pick_image(&manifest, "edel-vm-x86_64", "2026.11.0", false).is_err());
        assert!(pick_image(&manifest, "edel-vm-x86_64", "2026.11.0", true).is_ok());
        assert!(pick_image(&manifest, "edel-laptop-x86_64", "0.1", false).is_err());
    }

    #[test]
    fn orders_versions_by_number() {
        assert_eq!(compare_versions("2026.10.2", "2026.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.1", "0.1"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.0", "0.1"), Ordering::Equal);
    }

    #[test]
    fn finds_the_image_beside_the_manifest() {
        assert_eq!(
            beside("http://10.0.2.2:8000/release.toml", "a.ext4.gz"),
            "http://10.0.2.2:8000/a.ext4.gz"
        );
        assert_eq!(
            beside("/media/usb/release.toml", "a.ext4"),
            "/media/usb/a.ext4"
        );
        assert_eq!(beside("release.toml", "a.ext4"), "a.ext4");
    }

    #[test]
    fn hashes_a_gzipped_image_as_it_lands_in_the_slot() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("edel-gz-hash-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (raw, gz) = (dir.join("img.ext4"), dir.join("img.ext4.gz"));
        fs::write(&raw, b"slot contents").unwrap();
        let mut enc = flate2::write::GzEncoder::new(
            File::create(&gz).unwrap(),
            flate2::Compression::default(),
        );
        enc.write_all(b"slot contents").unwrap();
        enc.finish().unwrap();
        let plain = sha256_reader(&mut open_image(raw.to_str().unwrap()).unwrap()).unwrap();
        let packed = sha256_reader(&mut open_image(gz.to_str().unwrap()).unwrap()).unwrap();
        assert_eq!(plain, packed);
        assert_eq!(plain.1, 13);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(from_hex(&to_hex(&[0, 15, 255])).unwrap(), [0, 15, 255]);
        assert!(from_hex("abc").is_err());
    }
}
