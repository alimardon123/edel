//! Signed releases (roadmap M1.6). `release.toml` lists the images of one
//! release with their sha256 and size; `release.toml.sig` is an ed25519
//! signature over its exact bytes. Every image carries two public keys in
//! `/usr/share/edel/keys/`, so a key can be replaced without stranding a
//! machine. `edel update` refuses a manifest no key signed, an
//! image whose sha256 differs and a version that is not newer, and since
//! M3.8 a list past its `expires` date, one for another channel than the
//! machine's and a plain `http://` location, so a stale or crossed list
//! cannot hold a machine back.
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
use edel::i18n::{tr, trf};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The only manifest format this edel reads.
pub const FORMAT: i64 = 1;
/// Where images keep the public keys that may sign their updates.
pub const KEYS_DIR: &str = edel::places::KEYS_DIR;

/// `release.toml`. No `deny_unknown_fields`: a newer release may add keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: i64,
    pub version: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub date: String,
    /// The last day, `YYYY-MM-DD` in UTC, on which the list may be used
    /// (M3.8); set when it is signed. A list from before M3.8 has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expires: String,
    #[serde(default)]
    pub images: Vec<ImageEntry>,
}

/// What `edel update` accepts beyond a good signature (M3.8).
#[derive(Debug, Clone, Default)]
pub struct Accept {
    /// A version that is not newer than the running one.
    pub allow_downgrade: bool,
    /// A list for this channel rather than the machine's (`--channel`).
    pub channel: Option<String>,
    /// Plain `http://`, for a local test server (`--allow-http`).
    pub allow_http: bool,
}

/// Where the release lists of channels are served: a channel's is
/// `CHANNEL_LISTS/NAME/release.toml` (`ci/channel.sh` puts `stable`'s there
/// on the docs site, M3.4).
pub const CHANNEL_LISTS: &str = "https://alimardon123.github.io/edel/channels";

/// Where the preview's list is: a file of the rolling `preview`
/// pre-release (`ci/release.sh`), which every merge replaces.
pub const PREVIEW_LIST: &str =
    "https://github.com/alimardon123/edel/releases/download/preview/release.toml";

/// The address of `channel`'s release list.
pub fn channel_list(channel: &str) -> String {
    match channel {
        "preview" => PREVIEW_LIST.to_string(),
        _ => format!("{CHANNEL_LISTS}/{channel}/release.toml"),
    }
}

/// The channel this machine follows ([`edel::settings::channel`]), from
/// its settings files and the channel its image was built for.
pub fn machine_channel() -> String {
    let read = |path: PathBuf| fs::read_to_string(edel::places::found(&path)).ok();
    let machine = read(edel::places::machine_settings());
    let person = edel::places::person_settings().and_then(read);
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    edel::settings::channel(
        machine.as_deref(),
        person.as_deref(),
        os_release_value(&os_release, "EDEL_CHANNEL").as_deref(),
    )
}

/// The release list to take when none is named: `--channel`'s channel
/// once, else the machine's (M5.8c). A channel the address cannot hold is
/// refused before anything is fetched.
pub fn default_location(accept: &Accept) -> Result<String> {
    let channel = match accept.channel.as_deref() {
        Some(wanted) if !edel::settings::is_channel(wanted) => bail!(
            "{}",
            trf(
                "{channel} is not a channel name; use lowercase letters, digits and hyphens, such as \"stable\" or \"preview\"",
                &[("channel", &format!("{wanted:?}"))]
            )
        ),
        Some(wanted) => wanted.to_string(),
        None => machine_channel(),
    };
    Ok(channel_list(&channel))
}

/// Refuses a plain `http://` location unless `allow_http`: the signature
/// already covers what comes over it, but https keeps a list from being
/// swapped or held back on the way (M3.8).
fn check_location(location: &str, allow_http: bool) -> Result<()> {
    if location.starts_with("http://") && !allow_http {
        bail!(
            "{}",
            trf(
                "refused: http: {location} is plain http; use an https:// address, or pass --allow-http for a local test server",
                &[("location", location)]
            )
        );
    }
    Ok(())
}

/// Refuses a list past its `expires` date (`today` in days since
/// 1970-01-01, UTC) or for another channel than `channel`, the machine's,
/// unless `accept` asks for that channel.
fn check_list(manifest: &Manifest, today: i64, channel: &str, accept: &Accept) -> Result<()> {
    if !manifest.expires.is_empty() {
        let Some(last) = parse_date(&manifest.expires) else {
            bail!(
                "{}",
                trf(
                    "refused: expired: release.toml's expires {date} is not a date (YYYY-MM-DD); nothing was changed",
                    &[("date", &format!("{:?}", manifest.expires))]
                )
            );
        };
        if today > last {
            bail!(
                "{}",
                trf(
                    "refused: expired: this release list expired on {date}: the server may be stale, or replaying an old list; nothing was changed. If that date has not passed, check this machine's clock (date)",
                    &[("date", &manifest.expires)]
                )
            );
        }
    }
    let wanted = accept.channel.as_deref().unwrap_or(channel);
    if !manifest.channel.is_empty() && manifest.channel != wanted {
        bail!(
            "{}",
            trf(
                "refused: channel: this release list is for the {channel} channel, and this machine follows {wanted}; pass --channel {channel} to take it once, or set updates.channel to change for good",
                &[("channel", &manifest.channel), ("wanted", wanted)]
            )
        );
    }
    Ok(())
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// days_from_civil).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01, as `YYYY-MM-DD`.
fn civil_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DD` as days since 1970-01-01.
fn parse_date(text: &str) -> Option<i64> {
    let mut parts = text.trim().splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

/// Today, in days since 1970-01-01, by this machine's clock.
fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (secs / 86_400) as i64
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
    let text =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
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
            "{}",
            trf(
                "no public keys in {path}, so this system cannot check who signed an update; nothing was changed; reinstall from an Edel OS image",
                &[("path", &dir.display().to_string())]
            )
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
    bail!(
        "{}",
        tr(
            "refused: signature: no key this system trusts signed this release list; nothing was changed; check the address, or ask the publisher for a fresh list"
        )
    )
}

/// Parses a manifest, refusing only a format this edel does not know.
pub fn parse_manifest(text: &str) -> Result<Manifest> {
    let table: toml::Table = toml::from_str(text).context(tr("refused: release.toml is not valid TOML, so it is cut short or is not a release list; check the address"))?;
    let format = table
        .get("format")
        .and_then(|v| v.as_integer())
        .context(tr("refused: release.toml has no format line, so it is not a release list this edel can read; check the address"))?;
    if format != FORMAT {
        bail!(
            "{}",
            trf(
                "refused: release.toml is format {format}; this edel reads format {current}, so install a release in between first",
                &[
                    ("format", &format.to_string()),
                    ("current", &FORMAT.to_string())
                ]
            )
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
            "{}",
            trf(
                "refused: version {version} is not newer than the running {running}; pass --allow-downgrade to install it anyway",
                &[("version", &manifest.version), ("running", running)]
            )
        );
    }
    manifest
        .images
        .iter()
        .find(|e| e.name == image)
        .with_context(|| {
            trf(
                "refused: release {version} has no image {image}, the one this system is; nothing was changed, so wait for a release that lists it",
                &[("version", &manifest.version), ("image", &format!("{image:?}"))],
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

/// What a download that failed says: the address and what to try; the
/// cause follows it.
fn download_failed(location: &str) -> String {
    trf(
        "could not download {location}; check the network and the address, then run edel update again (the running system is unchanged)",
        &[("location", location)],
    )
}

/// `could not read PATH`, the start of a failure to read a file.
fn could_not_read(path: &str) -> String {
    trf("could not read {path}", &[("path", path)])
}

/// Reads a small file (a manifest or a signature) from a path or a URL.
fn fetch(location: &str) -> Result<Vec<u8>> {
    if is_url(location) {
        let mut response = agent()
            .get(location)
            .call()
            .with_context(|| download_failed(location))?;
        Ok(response.body_mut().read_to_vec()?)
    } else {
        let path = local_path(location);
        fs::read(path).with_context(|| could_not_read(path))
    }
}

/// Opens an image for streaming, decompressing a `.gz` file on the way.
fn open_image(location: &str) -> Result<Box<dyn Read>> {
    let raw: Box<dyn Read> = if is_url(location) {
        let response = agent()
            .get(location)
            .call()
            .with_context(|| download_failed(location))?;
        Box::new(response.into_body().into_reader())
    } else {
        let path = local_path(location);
        Box::new(File::open(path).with_context(|| could_not_read(path))?)
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
/// signature is checked against the keys this image carries and the list
/// against its date, the machine's channel and `accept` (M3.8).
fn verified_manifest(location: &str, accept: &Accept) -> Result<Manifest> {
    check_location(location, accept.allow_http)?;
    let bytes = fetch(location)?;
    let sig = fetch(&format!("{location}.sig"))
        .context(tr("refused: signature: could not read the .sig file beside release.toml; it must sit next to the list with the same name and .sig"))?;
    verify_bytes(
        &bytes,
        &String::from_utf8_lossy(&sig),
        &load_keys(Path::new(KEYS_DIR))?,
    )?;
    let manifest = parse_manifest(&String::from_utf8_lossy(&bytes))?;
    check_list(&manifest, today(), &machine_channel(), accept)?;
    Ok(manifest)
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
pub fn open_checked(location: &str, accept: &Accept) -> Result<Checked> {
    let manifest = verified_manifest(location, accept)?;
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let image = os_release_value(&os_release, "EDEL_IMAGE")
        .context(tr("this system's /usr/lib/os-release has no EDEL_IMAGE, so edel cannot tell which image of the release to take"))?;
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let entry = pick_image(&manifest, &image, &running, accept.allow_downgrade)?;
    let sha256 = from_hex(&entry.sha256).context(tr("refused: release.toml has a sha256 for the image that is not 64 hex digits; nothing was changed"))?;
    let image_at = beside(location, &entry.file);
    check_location(&image_at, accept.allow_http)?;
    println!(
        "edel update: {}",
        trf(
            "release {version}: signature checked",
            &[("version", &manifest.version)]
        )
    );
    Ok(Checked {
        sha256,
        size: entry.size,
        reader: open_image(&image_at)?,
    })
}

/// `edel update --check`: the running version and the one at `location`.
pub fn check(location: &str, accept: &Accept) -> Result<()> {
    let manifest = verified_manifest(location, accept)?;
    let os_release = fs::read_to_string("/usr/lib/os-release").unwrap_or_default();
    let running = os_release_value(&os_release, "VERSION_ID").unwrap_or_default();
    let newer = compare_versions(&manifest.version, &running) == Ordering::Greater;
    println!("running: {running}");
    println!(
        "available: {} ({})",
        manifest.version,
        if newer { tr("newer") } else { tr("not newer") }
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
        .with_context(|| format!("could not create {}", secret.display()))?;
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
        bail!(
            "--base-url {url:?} is not an http(s) URL; write one such as https://example.org/edel"
        );
    }
    let Some(dir) = images.first().and_then(|i| i.parent()) else {
        bail!("no image given; name at least one image after the options");
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
        expires: String::new(),
        images: entries,
    };
    let out = out_dir.unwrap_or(dir);
    fs::create_dir_all(out)?;
    let path = out.join("release.toml");
    fs::write(&path, toml::to_string(&manifest)?)?;
    println!("{}", path.display());
    Ok(path)
}

/// `edel release sign`: writes `FILE.sig`; with `expires_in`, first sets
/// the list's `expires` that many days from today (M3.8), so the date is
/// the signer's, not the build's.
pub fn sign(key_path: &Path, file: &Path, expires_in: Option<u32>) -> Result<()> {
    let text = fs::read_to_string(key_path)
        .with_context(|| format!("could not read {}", key_path.display()))?;
    let key = SigningKey::from_bytes(&fixed::<32>(from_hex(&text)?, "the secret key")?);
    if let Some(days) = expires_in {
        let list = fs::read_to_string(file)
            .with_context(|| format!("could not read {}", file.display()))?;
        let date = civil_from_days(today() + i64::from(days));
        fs::write(file, with_expires(&list, &date)?)?;
        println!("{}: expires {date}", file.display());
    }
    let bytes = fs::read(file).with_context(|| format!("could not read {}", file.display()))?;
    let sig = key.sign(&bytes);
    let sig_path = PathBuf::from(format!("{}.sig", file.display()));
    fs::write(&sig_path, format!("{}\n", to_hex(&sig.to_bytes())))?;
    println!("{}", sig_path.display());
    Ok(())
}

/// `list` with its `expires` set to `date`, every other line kept.
fn with_expires(list: &str, date: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = list.parse().context("the list is not valid TOML")?;
    doc["expires"] = toml_edit::value(date);
    // Before the [[images]] tables, as a top-level key must be.
    Ok(doc.to_string())
}

/// `edel release verify`: checks `FILE.sig` against the keys in `keys`.
pub fn verify(keys: &Path, file: &Path) -> Result<()> {
    let bytes = fs::read(file).with_context(|| format!("could not read {}", file.display()))?;
    let sig = fs::read_to_string(format!("{}.sig", file.display()))?;
    verify_bytes(&bytes, &sig, &load_keys(keys)?)?;
    println!("{}: signature good", file.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal comes out in a made-up language ("xx", M5.24b) with its
    /// values filled in, and in English where the catalogue lacks it.
    #[test]
    fn a_refusal_is_translated_and_falls_back_to_english() {
        let xx = edel::i18n::parse_po(
            "msgid \"refused: http: {location} is plain http; use an https:// address, or pass --allow-http for a local test server\"\n\
             msgstr \"xx: {location} xx\"\n",
        );
        edel::i18n::with_catalogue(xx.clone(), || {
            let refused = check_location("http://mirror.example/release.toml", false)
                .unwrap_err()
                .to_string();
            assert_eq!(refused, "xx: http://mirror.example/release.toml xx");
            // Another refusal, which this catalogue lacks, stays English.
            let old = Manifest {
                format: FORMAT,
                version: "2020.1.1".into(),
                channel: String::new(),
                date: String::new(),
                expires: "2020-01-01".into(),
                images: Vec::new(),
            };
            let late = check_list(
                &old,
                days_from_civil(2026, 10, 7),
                "stable",
                &Accept::default(),
            );
            assert!(
                late.unwrap_err()
                    .to_string()
                    .starts_with("refused: expired:")
            );
        });
        assert!(
            check_location("http://mirror.example/release.toml", false)
                .unwrap_err()
                .to_string()
                .starts_with("refused: http: http://mirror.example")
        );
    }

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

    #[test]
    fn a_channel_names_its_release_list() {
        assert_eq!(
            channel_list("stable"),
            "https://alimardon123.github.io/edel/channels/stable/release.toml"
        );
        assert_eq!(
            channel_list("preview"),
            "https://github.com/alimardon123/edel/releases/download/preview/release.toml",
            "the preview is the rolling pre-release's file"
        );
        assert_eq!(
            channel_list("fleet-1"),
            "https://alimardon123.github.io/edel/channels/fleet-1/release.toml"
        );
        let once = Accept {
            channel: Some("preview".into()),
            ..Accept::default()
        };
        assert_eq!(
            default_location(&once).unwrap(),
            channel_list("preview"),
            "--channel alone takes that channel's list once"
        );
        let bad = Accept {
            channel: Some("../x".into()),
            ..Accept::default()
        };
        assert_eq!(
            default_location(&bad).unwrap_err().to_string(),
            "\"../x\" is not a channel name; use lowercase letters, digits and hyphens, such as \"stable\" or \"preview\""
        );
        let machine = default_location(&Accept::default()).unwrap();
        assert!(machine.starts_with("https://") && machine.ends_with("/release.toml"));
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
    fn dates_round_trip_through_days() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("2026-10-07"), Some(20_733));
        assert_eq!(civil_from_days(20_733), "2026-10-07");
        assert_eq!(
            civil_from_days(parse_date("2028-02-29").unwrap() + 1),
            "2028-03-01"
        );
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("soon"), None);
    }

    #[test]
    fn refuses_an_expired_list_another_channel_and_plain_http() {
        let list = |expires: &str, channel: &str| {
            let mut m = parse_manifest(MANIFEST).unwrap();
            m.expires = expires.into();
            m.channel = channel.into();
            m
        };
        let today = parse_date("2026-10-07").unwrap();
        let none = Accept::default();
        assert!(check_list(&list("2026-10-07", "stable"), today, "stable", &none).is_ok());
        let err = check_list(&list("2026-10-06", "stable"), today, "stable", &none).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("refused: expired: this release list expired on 2026-10-06"),
            "{err}"
        );
        // A list from before M3.8 has no date and is still read.
        assert!(check_list(&list("", "stable"), today, "stable", &none).is_ok());
        let err = check_list(&list("", "preview"), today, "stable", &none).unwrap_err();
        assert!(
            err.to_string().contains("this release list is for the preview channel, and this machine follows stable; pass --channel preview"),
            "{err}"
        );
        let asked = Accept {
            channel: Some("preview".into()),
            ..Accept::default()
        };
        assert!(check_list(&list("", "preview"), today, "stable", &asked).is_ok());
        let err = check_location("http://10.0.2.2:8000/release.toml", false).unwrap_err();
        assert!(err.to_string().starts_with("refused: http:"), "{err}");
        assert!(check_location("http://10.0.2.2:8000/release.toml", true).is_ok());
        assert!(check_location("https://example.org/release.toml", false).is_ok());
        assert!(check_location("/srv/release.toml", false).is_ok());
    }

    #[test]
    fn signing_sets_the_date_a_list_expires_before_its_images() {
        let text = with_expires(MANIFEST, "2026-11-21").unwrap();
        let m = parse_manifest(&text).unwrap();
        assert_eq!(m.expires, "2026-11-21");
        assert_eq!(m.images.len(), 1);
        assert!(text.find("expires").unwrap() < text.find("[[images]]").unwrap());
        // Signing again replaces the date.
        let again = parse_manifest(&with_expires(&text, "2026-12-01").unwrap()).unwrap();
        assert_eq!(again.expires, "2026-12-01");
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

    /// Each way an update is refused says what failed, why and what to do,
    /// word for word (docs/MESSAGES.md); CI's A/B test greps the
    /// `refused: KIND` start of the first two.
    #[test]
    fn every_refusal_says_what_failed_why_and_what_to_do() {
        let keys = [key(1).verifying_key()];
        let wrong = sig(&key(9), MANIFEST);
        assert_eq!(
            verify_bytes(MANIFEST.as_bytes(), &wrong, &keys)
                .unwrap_err()
                .to_string(),
            "refused: signature: no key this system trusts signed this release list; nothing was changed; check the address, or ask the publisher for a fresh list"
        );
        let mut list = parse_manifest(MANIFEST).unwrap();
        list.expires = "2026-10-06".into();
        let today = parse_date("2026-10-07").unwrap();
        assert_eq!(
            check_list(&list, today, "stable", &Accept::default())
                .unwrap_err()
                .to_string(),
            "refused: expired: this release list expired on 2026-10-06: the server may be stale, or replaying an old list; nothing was changed. If that date has not passed, check this machine's clock (date)"
        );
        assert_eq!(
            check_list(&list, today - 9, "preview", &Accept::default())
                .unwrap_err()
                .to_string(),
            "refused: channel: this release list is for the stable channel, and this machine follows preview; pass --channel stable to take it once, or set updates.channel to change for good"
        );
        assert_eq!(
            check_location("http://10.0.2.2:8000/release.toml", false)
                .unwrap_err()
                .to_string(),
            "refused: http: http://10.0.2.2:8000/release.toml is plain http; use an https:// address, or pass --allow-http for a local test server"
        );
        let manifest = parse_manifest(MANIFEST).unwrap();
        assert_eq!(
            pick_image(&manifest, "edel-vm-x86_64", "2026.10.2", false)
                .unwrap_err()
                .to_string(),
            "refused: version 2026.10.2 is not newer than the running 2026.10.2; pass --allow-downgrade to install it anyway"
        );
        assert_eq!(
            pick_image(&manifest, "edel-laptop-x86_64", "2026.10.1", false)
                .unwrap_err()
                .to_string(),
            "refused: release 2026.10.2 has no image \"edel-laptop-x86_64\", the one this system is; nothing was changed, so wait for a release that lists it"
        );
        assert_eq!(
            parse_manifest(&MANIFEST.replace("format = 1", "format = 2"))
                .unwrap_err()
                .to_string(),
            "refused: release.toml is format 2; this edel reads format 1, so install a release in between first"
        );
        assert_eq!(
            download_failed("https://example.org/release.toml"),
            "could not download https://example.org/release.toml; check the network and the address, then run edel update again (the running system is unchanged)"
        );
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(from_hex(&to_hex(&[0, 15, 255])).unwrap(), [0, 15, 255]);
        assert!(from_hex("abc").is_err());
    }
}
