//! A theme as one file, `NAME.edeltheme` (M5.12b6, ADR-007 level 1): the
//! theme's table as the settings file holds it, and its pictures, as one
//! archive that `tar tf` reads.
//!
//! The archive is an uncompressed POSIX ustar archive, written and read
//! here by this module, so no dependency is added: the pictures are
//! compressed already, and ustar is a header of 512 bytes per entry. Its
//! entries, in this order: `theme.toml`, which holds `format = 1`, `name =
//! "NAME"` and the theme's table laid flat at the top level (its `title`,
//! its `pictures` and its appearance values, as `[themes.NAME]` holds them);
//! then `pictures/FILE` for each picture the theme's own paths name. Nothing
//! else is read: no link, no device, no folder but `pictures/`, no absolute
//! path, no `..`, no name outside `[A-Za-z0-9._-]`, no entry twice, and no
//! picture that is not a JPEG or PNG by its first bytes. Nothing in the file
//! runs, and nothing from it is written outside the theme's own folder
//! ([`Theme::install`]).
//!
//! The theme's own checker judges `theme.toml`: it is placed as
//! `[themes.NAME]` in a settings text and [`settings::check`] runs on it, so
//! a file is refused by the same rules as a settings file.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use toml::{Table, Value};

use crate::i18n::{tr, trf};
use crate::settings::{self, OwnTheme};
use crate::themes;

/// The file's extension, without the dot.
pub const EXTENSION: &str = "edeltheme";
/// The format of `theme.toml`, which this release writes and reads.
pub const FORMAT: i64 = 1;
/// The archive's first entry.
pub const THEME_FILE: &str = "theme.toml";
/// The folder of an archive's pictures; each entry is `pictures/FILE`.
pub const PICTURES: &str = "pictures/";
/// The most pictures one file holds.
pub const MOST_PICTURES: usize = 64;
/// The most bytes one picture may have: 32 MiB.
pub const MOST_PICTURE_BYTES: u64 = 32 * 1024 * 1024;
/// The most bytes one file may have: 256 MiB.
pub const MOST_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// The size of a tar block: a header, or 512 bytes of data.
const BLOCK: usize = 512;
/// The most bytes a name takes in a header's name field.
const NAME_FIELD: usize = 100;
/// The longest stem a picture's file name keeps when exported, so that
/// `pictures/` and the extension still fit the name field.
const STEM_LIMIT: usize = 80;
/// The root a theme's picture points at while the theme is checked: it is
/// not a real place, and it passes the checker's form for a picture's path.
const CHECK_ROOT: &str = "/edel-theme-file";
/// The name a theme is checked under when it is a built-in one's, which
/// the checker would refuse as a person's own name.
const CHECK_NAME: &str = "archived-theme";

/// A theme as the file holds it.
#[derive(Debug, Clone)]
pub struct Theme {
    /// The name the file gives the theme (a valid name, not yet free).
    pub name: String,
    /// The theme's table, its pictures as `pictures/FILE`.
    pub table: OwnTheme,
    /// The pictures the file holds, each once.
    pub pictures: Vec<Picture>,
}

/// One picture of a theme file.
#[derive(Debug, Clone)]
pub struct Picture {
    /// Its file name: letters, digits, dots, hyphens and underscores.
    pub file: String,
    /// Its bytes, a JPEG or PNG.
    pub bytes: Vec<u8>,
}

impl Theme {
    /// The theme's table with each picture's reference as its full path in
    /// `dir`, the folder [`Theme::install`] writes to. Nothing is written.
    pub fn at(&self, dir: &Path) -> OwnTheme {
        let mut table = self.table.clone();
        for item in &mut table.pictures {
            if let Some(file) = item.strip_prefix(PICTURES) {
                *item = dir.join(file).display().to_string();
            }
        }
        table
    }

    /// Writes the pictures into `dir` (made first), each as `FILE.new` and
    /// then renamed over `FILE`; returns how many were written.
    pub fn install(&self, dir: &Path) -> Result<usize> {
        for picture in &self.pictures {
            write_file(&dir.join(&picture.file), &picture.bytes)?;
        }
        Ok(self.pictures.len())
    }
}

/// Reads a theme file and checks it, every refusal of the format before any
/// of its pictures is used.
pub fn read(path: &Path) -> Result<Theme> {
    let shown = path.display().to_string();
    let size = fs::metadata(path)
        .with_context(|| could_not_read(&shown))?
        .len();
    if size > MOST_FILE_BYTES {
        bail!("{}", too_big());
    }
    let bytes = fs::read(path).with_context(|| could_not_read(&shown))?;
    unpack(&bytes)
}

/// Checks the bytes of a theme file and reads the theme in them.
pub fn unpack(bytes: &[u8]) -> Result<Theme> {
    if bytes.len() as u64 > MOST_FILE_BYTES {
        bail!("{}", too_big());
    }
    let mut entries: Vec<(Entry, Vec<u8>)> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut at = 0usize;
    loop {
        let Some(header) = bytes.get(at..at + BLOCK) else {
            if at == 0 {
                bail!("{}", not_a_theme_file());
            }
            bail!("{}", cut_short());
        };
        if header.iter().all(|&b| b == 0) {
            // The end of the archive: nothing may follow it.
            if bytes[at + BLOCK..].iter().any(|&b| b != 0) {
                bail!(
                    "{}",
                    tr("the theme file has data after its end; copy it again")
                );
            }
            break;
        }
        if !is_ustar(header) {
            if at == 0 {
                bail!("{}", not_a_theme_file());
            }
            bail!(
                "{}",
                tr("the theme file has a damaged header; copy it again")
            );
        }
        let name = header_name(header)?;
        let size = octal(&header[124..136]).and_then(|size| usize::try_from(size).ok());
        let Some(size) = size else {
            bail!(
                "{}",
                tr("the theme file has a damaged header; copy it again")
            );
        };
        let kind = header[156];
        at += BLOCK;
        let Some(data) = at.checked_add(size).and_then(|end| bytes.get(at..end)) else {
            bail!("{}", cut_short());
        };
        let entry = entry_of(kind, &name)?;
        if !seen.insert(name.clone()) {
            bail!(
                "{}",
                trf(
                    "{name} is in the file twice; a theme file holds each entry once",
                    &[("name", &name)]
                )
            );
        }
        at += size.div_ceil(BLOCK) * BLOCK;
        if at > bytes.len() {
            bail!("{}", cut_short());
        }
        entries.push((entry, data.to_vec()));
    }
    let theme_at = entries
        .iter()
        .position(|(entry, _)| matches!(entry, Entry::Theme))
        .context(not_in_file())?;
    if theme_at != 0 {
        bail!(
            "{}",
            tr("theme.toml is not the first entry; copy the file again")
        );
    }
    let mut pictures = Vec::new();
    for (entry, data) in entries.iter().skip(1) {
        let Entry::Picture(file) = entry else {
            continue;
        };
        if data.len() as u64 > MOST_PICTURE_BYTES {
            bail!(
                "{}",
                trf(
                    "pictures/{file} is over 32 MiB; a theme picture is at most 32 MiB",
                    &[("file", file)]
                )
            );
        }
        if !is_picture(data) {
            bail!(
                "{}",
                trf(
                    "pictures/{file} is not a JPEG or PNG picture; a theme holds only those",
                    &[("file", file)]
                )
            );
        }
        pictures.push(Picture {
            file: file.clone(),
            bytes: data.clone(),
        });
    }
    if pictures.len() > MOST_PICTURES {
        bail!(
            "{}",
            trf(
                "a theme file holds up to {most} pictures; take some out of the theme",
                &[("most", &MOST_PICTURES.to_string())]
            )
        );
    }
    let (name, table, refs) = parse_theme(&entries[0].1)?;
    for file in &refs {
        if !pictures.iter().any(|picture| &picture.file == file) {
            bail!(
                "{}",
                trf(
                    "theme.toml names pictures/{file}, which the file does not hold; export the theme again",
                    &[("file", file)]
                )
            );
        }
    }
    for picture in &pictures {
        if !refs.contains(&picture.file) {
            bail!(
                "{}",
                trf(
                    "pictures/{file} is in the file but no theme names it; export the theme again",
                    &[("file", &picture.file)]
                )
            );
        }
    }
    Ok(Theme {
        name,
        table,
        pictures,
    })
}

/// Makes the theme file of `theme` (named `name`, as the theme is kept)
/// from the theme in use's table: each picture of the theme that is a path
/// on this machine is read into the file as `pictures/FILE`, and each name
/// or colour stays as it is. Refuses a picture that is missing, too big or
/// not a JPEG or PNG; the bytes are read back through [`unpack`] before they
/// are returned, so a file that would not import is never written.
pub fn pack(name: &str, theme: &OwnTheme) -> Result<Vec<u8>> {
    if !themes::is_name(name) {
        bail!("{}", themes::name_message(name));
    }
    let Value::Table(mut table) = Value::try_from(theme)? else {
        bail!("{}", tr("a theme could not be written as a table"));
    };
    let mut pictures: Vec<Picture> = Vec::new();
    let mut sources: Vec<(String, String)> = Vec::new();
    let mut items = Vec::new();
    for item in theme.pictures.iter() {
        if item.starts_with('/') {
            let file = bundle(item, &mut pictures, &mut sources)?;
            items.push(Value::String(format!("{PICTURES}{file}")));
        } else {
            items.push(Value::String(item.clone()));
        }
    }
    if theme.pictures.is_empty() {
        table.remove("pictures");
    } else {
        table.insert("pictures".into(), Value::Array(items));
    }
    let mut out = Vec::new();
    push_file(&mut out, THEME_FILE, theme_toml(name, table)?.as_bytes());
    for picture in &pictures {
        push_file(
            &mut out,
            &format!("{PICTURES}{}", picture.file),
            &picture.bytes,
        );
    }
    out.extend_from_slice(&[0u8; 2 * BLOCK]);
    unpack(&out).with_context(|| {
        tr("the theme would not read back as a theme file, so no file was written")
    })?;
    Ok(out)
}

/// Writes theme `name`, with `theme`'s table, as a theme file at `path`.
/// A file that is there already is never written over, and nothing is
/// written when the theme would not read back ([`pack`]).
pub fn export(name: &str, theme: &OwnTheme, path: &Path) -> Result<()> {
    let shown = path.display().to_string();
    if path.exists() {
        bail!(
            "{}",
            trf(
                "{file} is there already; give another file name",
                &[("file", &shown)]
            )
        );
    }
    let bytes = pack(name, theme)?;
    write_file(path, &bytes)
}

/// An import, planned before anything is written: the name the theme takes
/// in the person's file, its table with its pictures where they will be,
/// and what it sets, for the dialog that offers it. [`plan`] checks the
/// table against the settings text first, so a refused theme writes no
/// picture and no line.
pub struct Import {
    /// The name the theme takes: its file's name when that is free.
    pub name: String,
    /// The title the theme shows: its own, else its name made readable.
    pub title: String,
    /// The table, with each picture's reference at its full path in the
    /// person's themes folder.
    pub table: OwnTheme,
    /// The appearance keys the theme sets, as `appearance.accent`.
    pub keys: Vec<String>,
    /// How many pictures the file brings.
    pub pictures: usize,
    theme: Theme,
    dir: PathBuf,
}

/// Plans the import of `theme` into the settings text `text` (an empty text
/// is a file that does not exist yet): the name it takes, where its pictures
/// go, and the check of the table, which fails for a theme the file refuses.
pub fn plan(theme: Theme, text: &str, themes_dir: &Path) -> Result<Import> {
    let file = settings::read(&or_empty(text))?.file;
    let name = themes::import_name(&theme.name, |taken| file.themes.contains_key(taken));
    let dir = themes_dir.join(&name);
    let table = theme.at(&dir);
    let import = Import {
        title: table
            .title
            .clone()
            .unwrap_or_else(|| themes::title(&name, &BTreeMap::new())),
        keys: themes::value_keys(&table),
        pictures: theme.pictures.len(),
        name,
        table,
        theme,
        dir,
    };
    themes::add_text(&or_empty(text), &import.name, &import.table)?;
    Ok(import)
}

impl Import {
    /// Copies the pictures into the person's themes folder under the theme's
    /// name, each as `FILE.new` then renamed; returns how many were written.
    pub fn install(&self) -> Result<usize> {
        self.theme.install(&self.dir)
    }

    /// The settings text with the theme added under its name, as
    /// [`themes::add_text`] adds it: the theme in use is not changed.
    pub fn add(&self, text: &str) -> Result<String> {
        themes::add_text(&or_empty(text), &self.name, &self.table)
    }
}

/// A settings file's text, or the format line of a file that does not exist.
fn or_empty(text: &str) -> String {
    if text.trim().is_empty() {
        format!("format = {}\n", settings::FORMAT)
    } else {
        text.to_string()
    }
}

/// Writes `bytes` to `path` whole, after making its folder, so a reader
/// never sees half a file (`edel::files`).
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::files::replace(path, bytes).with_context(|| could_not_write(&path.display().to_string()))
}

/// What an archive entry is, once its name and type are judged.
enum Entry {
    /// `theme.toml`.
    Theme,
    /// `pictures/FILE`, with its file name.
    Picture(String),
    /// The folder `pictures/`, which holds nothing of its own.
    Folder,
}

/// Judges an entry by its type and name, naming the problem in the message.
fn entry_of(kind: u8, name: &str) -> Result<Entry> {
    match kind {
        b'1' | b'2' => bail!(
            "{}",
            trf(
                "{name} is a link; a theme file holds only files and its pictures folder, so it is refused",
                &[("name", name)]
            )
        ),
        b'3' | b'4' | b'6' => bail!(
            "{}",
            trf(
                "{name} is a device or a pipe; a theme file holds only files, so it is refused",
                &[("name", name)]
            )
        ),
        b'0' | 0 | b'5' => {}
        _ => bail!("{}", outside(name)),
    }
    if name.starts_with('/') {
        bail!(
            "{}",
            trf(
                "{name} is an absolute path; a theme file holds only its own entries, relative to it",
                &[("name", name)]
            )
        );
    }
    if name.split('/').any(|part| part == "..") {
        bail!(
            "{}",
            trf(
                "{name} has .. in its path; a theme file holds only its own entries",
                &[("name", name)]
            )
        );
    }
    if kind == b'5' {
        if name == PICTURES || name == PICTURES.trim_end_matches('/') {
            return Ok(Entry::Folder);
        }
        bail!(
            "{}",
            trf(
                "{name} is a folder; a theme file holds theme.toml and the pictures/ folder only",
                &[("name", name)]
            )
        );
    }
    if name == THEME_FILE {
        return Ok(Entry::Theme);
    }
    if let Some(file) = name.strip_prefix(PICTURES) {
        if !plain_name(file) {
            bail!(
                "{}",
                trf(
                    "{name} has a name a theme file may not have; a picture's file name uses only letters, digits, dots, hyphens and underscores",
                    &[("name", name)]
                )
            );
        }
        return Ok(Entry::Picture(file.to_string()));
    }
    bail!("{}", outside(name))
}

/// The message for an entry that is neither `theme.toml` nor a picture.
fn outside(name: &str) -> String {
    trf(
        "{name} is not part of a theme; a theme file holds theme.toml and pictures/ only",
        &[("name", name)],
    )
}

/// Whether `file` is a picture's file name: letters, digits, dots, hyphens
/// and underscores, and not `.` itself.
fn plain_name(file: &str) -> bool {
    !file.is_empty()
        && file != "."
        && file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// Reads `theme.toml` (the entry's bytes): its format and name, and its
/// table, checked by the settings checker as `[themes.NAME]`. Returns the
/// name, the table and the picture files its pictures name.
fn parse_theme(bytes: &[u8]) -> Result<(String, OwnTheme, Vec<String>)> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!(tr("theme.toml is not text; copy the file again")))?;
    let mut table: Table = toml::from_str(text).map_err(|err| {
        anyhow::anyhow!(trf(
            "theme.toml is not valid TOML: {err}",
            &[("err", err.message())]
        ))
    })?;
    match table.remove("format") {
        Some(Value::Integer(FORMAT)) => {}
        Some(other) => bail!(
            "{}",
            trf(
                "theme.toml is format {format}; this release reads format {current}",
                &[
                    ("format", &other.to_string()),
                    ("current", &FORMAT.to_string())
                ]
            )
        ),
        None => bail!(
            "{}",
            tr("theme.toml has no format = 1 line; export the theme again")
        ),
    }
    let name = match table.remove("name") {
        Some(Value::String(name)) if themes::is_name(&name) => name,
        Some(Value::String(name)) => bail!("{}", themes::name_message(&name)),
        _ => bail!("{}", tr("theme.toml has no name = \"NAME\" line")),
    };
    let mut refs = Vec::new();
    if let Some(Value::Array(items)) = table.get("pictures") {
        for item in items {
            let Some(value) = item.as_str() else {
                continue;
            };
            if value.starts_with('/') {
                bail!(
                    "{}",
                    trf(
                        "theme.toml names {value}, a picture's full path; a theme file names its pictures as pictures/FILE",
                        &[("value", value)]
                    )
                );
            }
            if let Some(file) = value.strip_prefix(PICTURES) {
                refs.push(file.to_string());
            }
        }
    }
    // The checker and the reader see each picture at a place that passes
    // the form of a picture's path, and the theme read back keeps the
    // references as the file has them. The check runs under a name no
    // built-in theme has: a built-in name is renamed on import, not refused.
    let placed = placed_pictures(&table, |file| format!("{CHECK_ROOT}/{PICTURES}{file}"));
    let themed = |name: &str| -> Result<String> {
        let mut root = Table::new();
        root.insert("format".into(), Value::Integer(settings::FORMAT));
        let mut themes_table = Table::new();
        themes_table.insert(name.to_string(), Value::Table(placed.clone()));
        root.insert("themes".into(), Value::Table(themes_table));
        Ok(toml::to_string(&root)?)
    };
    let problems = settings::check(&themed(CHECK_NAME)?)?;
    if !problems.is_empty() {
        bail!(
            "{}",
            trf(
                "theme.toml is not a theme this release reads: {problems}",
                &[("problems", &problems.join("; "))]
            )
        );
    }
    let read = settings::read(&themed(&name)?)?.file;
    let mut theme = read
        .themes
        .get(&name)
        .cloned()
        .with_context(|| tr("theme.toml does not hold the theme"))?;
    for item in &mut theme.pictures {
        if let Some(file) = item.strip_prefix(&format!("{CHECK_ROOT}/")) {
            *item = file.to_string();
        }
    }
    Ok((name, theme, refs))
}

/// `table` with each picture reference `pictures/FILE` replaced by `place`
/// of its file; other references and the other keys stay.
fn placed_pictures(table: &Table, place: impl Fn(&str) -> String) -> Table {
    let mut out = table.clone();
    if let Some(Value::Array(items)) = out.get_mut("pictures") {
        for item in items.iter_mut() {
            if let Some(file) = item.as_str().and_then(|v| v.strip_prefix(PICTURES)) {
                *item = Value::String(place(file));
            }
        }
    }
    out
}

/// Bundles the picture at `path` into `pictures` (once for each path) and
/// gives its file name in the archive, unique among the pictures.
fn bundle(
    path: &str,
    pictures: &mut Vec<Picture>,
    sources: &mut Vec<(String, String)>,
) -> Result<String> {
    if let Some((_, file)) = sources.iter().find(|(source, _)| source == path) {
        return Ok(file.clone());
    }
    let source = Path::new(path);
    let Some(meta) = fs::metadata(source).ok().filter(|meta| meta.is_file()) else {
        bail!(
            "{}",
            trf(
                "there is no picture at {path}, which the theme names; the theme cannot be exported until it is there",
                &[("path", path)]
            )
        );
    };
    if meta.len() > MOST_PICTURE_BYTES {
        bail!(
            "{}",
            trf(
                "{path} is over 32 MiB; a theme picture is at most 32 MiB",
                &[("path", path)]
            )
        );
    }
    let bytes = fs::read(source).with_context(|| could_not_read(path))?;
    if !is_picture(&bytes) {
        bail!(
            "{}",
            trf(
                "{path} is not a JPEG or PNG picture; a theme holds only those",
                &[("path", path)]
            )
        );
    }
    let file = unique_file_name(path, pictures);
    if pictures.len() == MOST_PICTURES {
        bail!(
            "{}",
            trf(
                "a theme file holds up to {most} pictures; take some out of the theme",
                &[("most", &MOST_PICTURES.to_string())]
            )
        );
    }
    sources.push((path.to_string(), file.clone()));
    pictures.push(Picture {
        file: file.clone(),
        bytes,
    });
    Ok(file)
}

/// The file name a picture of `path` takes in an archive: its own name with
/// the characters a file name may not have made hyphens, its stem at most
/// [`STEM_LIMIT`] characters, and `-2`, `-3` ... added to the stem when a
/// picture already holds that name.
fn unique_file_name(path: &str, pictures: &[Picture]) -> String {
    let base = Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = match base.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (base.clone(), String::new()),
    };
    let clean = |text: &str| -> String {
        let mut out = String::new();
        for c in text.chars() {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                out.push(c);
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
        out
    };
    let mut stem: String = clean(&stem)
        .trim_start_matches('.')
        .chars()
        .take(STEM_LIMIT)
        .collect();
    let ext = clean(&ext);
    if stem.is_empty() {
        stem = "picture".to_string();
    }
    let free = |file: &str| !pictures.iter().any(|picture| picture.file == file);
    std::iter::once(format!("{stem}{ext}"))
        .chain((2..).map(|n| format!("{stem}-{n}{ext}")))
        .find(|file| free(file))
        .unwrap_or_default()
}

/// The `theme.toml` text: `format` and `name`, then the table laid flat.
fn theme_toml(name: &str, table: Table) -> Result<String> {
    let mut root = Table::new();
    root.insert("format".into(), Value::Integer(FORMAT));
    root.insert("name".into(), Value::String(name.to_string()));
    root.extend(table);
    Ok(toml::to_string(&root)?)
}

/// Whether `bytes` begin as a JPEG or a PNG does.
fn is_picture(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

/// Whether a header block is a ustar one with a right checksum.
fn is_ustar(header: &[u8]) -> bool {
    if &header[257..262] != b"ustar" || !matches!(header[262], 0 | b' ') {
        return false;
    }
    let Some(stored) = octal(&header[148..156]) else {
        return false;
    };
    let unsigned: u64 = header
        .iter()
        .enumerate()
        .map(|(at, &b)| {
            if (148..156).contains(&at) {
                u64::from(b' ')
            } else {
                u64::from(b)
            }
        })
        .sum();
    stored == unsigned
}

/// The name a header gives: its prefix, a slash, then its name.
fn header_name(header: &[u8]) -> Result<String> {
    let name = std::str::from_utf8(text_field(&header[0..NAME_FIELD]))
        .map_err(|_| anyhow::anyhow!(tr("an entry's name is not text; copy the file again")))?;
    let prefix = std::str::from_utf8(text_field(&header[345..500]))
        .map_err(|_| anyhow::anyhow!(tr("an entry's name is not text; copy the file again")))?;
    Ok(if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    })
}

/// A header's text field up to its first NUL.
fn text_field(field: &[u8]) -> &[u8] {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    &field[..end]
}

/// An octal number in a header field, ended by a NUL or a space; None when
/// it holds anything else, or the top bit that asks for a binary size.
fn octal(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|&b| b & 0x80 != 0) {
        return None;
    }
    let mut value: u64 = 0;
    let mut digits = 0;
    for &b in field {
        match b {
            b'0'..=b'7' => {
                value = value.checked_mul(8)?.checked_add(u64::from(b - b'0'))?;
                digits += 1;
            }
            b' ' | 0 if digits > 0 || b == 0 => break,
            b' ' => continue,
            _ => return None,
        }
    }
    Some(value)
}

/// Appends one regular file's header, its bytes and their padding.
fn push_file(out: &mut Vec<u8>, name: &str, data: &[u8]) {
    out.extend_from_slice(&header(name, data.len() as u64));
    out.extend_from_slice(data);
    let pad = (BLOCK - data.len() % BLOCK) % BLOCK;
    out.extend(std::iter::repeat_n(0u8, pad));
}

/// A regular file's header block (`name` is at most [`NAME_FIELD`] bytes).
fn header(name: &str, size: u64) -> [u8; BLOCK] {
    let mut h = [0u8; BLOCK];
    let bytes = name.as_bytes();
    let n = bytes.len().min(NAME_FIELD);
    h[..n].copy_from_slice(&bytes[..n]);
    write_octal(&mut h[100..108], 0o644, 7);
    write_octal(&mut h[108..116], 0, 7);
    write_octal(&mut h[116..124], 0, 7);
    write_octal(&mut h[124..136], size, 11);
    write_octal(&mut h[136..148], 0, 11);
    h[148..156].fill(b' ');
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum: u64 = h.iter().map(|&b| u64::from(b)).sum();
    let check = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(check.as_bytes());
    h
}

/// `value` as octal digits in `field`, `digits` long, then a NUL.
fn write_octal(field: &mut [u8], value: u64, digits: usize) {
    let text = format!("{value:0digits$o}");
    field[..digits].copy_from_slice(text.as_bytes());
    field[digits] = 0;
}

/// `could not read PATH`, the start of a failure to read a file.
fn could_not_read(shown: &str) -> String {
    trf("could not read {path}", &[("path", shown)])
}

/// `could not write PATH`, the start of a failure to write a file.
fn could_not_write(shown: &str) -> String {
    trf("could not write {path}", &[("path", shown)])
}

/// The message for a file over [`MOST_FILE_BYTES`].
fn too_big() -> String {
    trf(
        "the theme file is over {most} MiB, more than a theme holds; take some pictures out of the theme and export it again",
        &[("most", &(MOST_FILE_BYTES / 1024 / 1024).to_string())],
    )
}

/// The message for bytes that are not a tar archive.
fn not_a_theme_file() -> String {
    tr("not a theme file: it is not a tar archive; export the theme with edel theme export, or Export in Settings")
        .to_string()
}

/// The message for an archive that stops before its end.
fn cut_short() -> String {
    tr("the theme file is cut short; copy it again").to_string()
}

/// The message for an archive with no `theme.toml`.
fn not_in_file() -> String {
    tr("the theme file has no theme.toml; export the theme again").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG's first bytes, then filler: a picture for the tests.
    fn jpeg(filler: usize) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
        bytes.extend(std::iter::repeat_n(7u8, filler));
        bytes
    }

    fn png(filler: usize) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend(std::iter::repeat_n(3u8, filler));
        bytes
    }

    /// A temp folder named after the test and the process.
    fn scratch(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("edel-theme-file-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A theme with two own pictures, a colour and a name, as `theme save`
    /// would keep one.
    fn own_theme(dir: &Path) -> OwnTheme {
        let one = dir.join("hills-day.jpg");
        let two = dir.join("sky.png");
        fs::write(&one, jpeg(100)).unwrap();
        fs::write(&two, png(100)).unwrap();
        let text = format!(
            "format = 1\n[themes.mine]\ntitle = \"Mine\"\npictures = [{:?}, {:?}, \"aurora\", \"#102030\"]\naccent = \"blue\"\nradius = 9\n",
            one.display().to_string(),
            two.display().to_string()
        );
        settings::read(&text)
            .unwrap()
            .file
            .themes
            .get("mine")
            .cloned()
            .unwrap()
    }

    /// Builds a ustar archive from entries of (name, type, bytes), with the
    /// end blocks, for the refusal tests.
    fn archive(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, kind, data) in entries {
            let mut h = header(name, data.len() as u64);
            h[156] = *kind;
            let sum: u64 = h
                .iter()
                .enumerate()
                .map(|(at, &b)| {
                    if (148..156).contains(&at) {
                        32
                    } else {
                        u64::from(b)
                    }
                })
                .sum();
            h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
            out.extend_from_slice(&h);
            out.extend_from_slice(data);
            out.extend(std::iter::repeat_n(
                0u8,
                (BLOCK - data.len() % BLOCK) % BLOCK,
            ));
        }
        out.extend_from_slice(&[0u8; 2 * BLOCK]);
        out
    }

    fn theme_toml_text(name: &str, pictures: &str) -> String {
        format!(
            "format = 1\nname = \"{name}\"\ntitle = \"Mine\"\npictures = [{pictures}]\naccent = \"blue\"\n"
        )
    }

    fn refused(bytes: &[u8]) -> String {
        format!("{:#}", unpack(bytes).unwrap_err())
    }

    #[test]
    fn a_theme_exports_and_imports_with_its_pictures_and_references_rewritten() {
        let dir = scratch("round-trip");
        let theme = own_theme(&dir);
        let bytes = pack("mine", &theme).unwrap();
        let read = unpack(&bytes).unwrap();
        assert_eq!(read.name, "mine");
        assert_eq!(read.pictures.len(), 2);
        assert_eq!(read.table.title.as_deref(), Some("Mine"));
        assert_eq!(read.table.values.accent.as_deref(), Some("blue"));
        assert_eq!(read.table.values.radius, Some(9));
        assert_eq!(read.table.pictures[0], "pictures/hills-day.jpg");
        assert_eq!(read.table.pictures[1], "pictures/sky.png");
        assert_eq!(read.table.pictures[2], "aurora");
        assert_eq!(read.table.pictures[3], "#102030");
        // A file on a machine that has none of the pictures: the copies
        // come back, and the references point at them.
        let home = dir.join("home");
        let target = home.join("mine");
        let copied = read.at(&target);
        assert_eq!(read.install(&target).unwrap(), 2);
        assert_eq!(
            copied.pictures[0],
            target.join("hills-day.jpg").display().to_string()
        );
        assert_eq!(fs::read(target.join("hills-day.jpg")).unwrap(), jpeg(100));
        assert_eq!(fs::read(target.join("sky.png")).unwrap(), png(100));
        assert!(!target.join("sky.png.new").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_built_in_theme_exports_as_its_table_alone() {
        let theme = themes::lookup("aurora", &BTreeMap::new()).unwrap();
        let read = unpack(&pack("aurora", &theme).unwrap()).unwrap();
        assert!(read.pictures.is_empty());
        assert_eq!(read.table.pictures, theme.pictures);
    }

    #[test]
    fn two_pictures_of_one_name_keep_both_with_a_number() {
        let dir = scratch("names");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("pic.jpg"), jpeg(10)).unwrap();
        fs::write(b.join("pic.jpg"), jpeg(20)).unwrap();
        let text = format!(
            "format = 1\n[themes.two]\npictures = [{:?}, {:?}]\n",
            a.join("pic.jpg").display().to_string(),
            b.join("pic.jpg").display().to_string()
        );
        let theme = settings::read(&text)
            .unwrap()
            .file
            .themes
            .get("two")
            .cloned()
            .unwrap();
        let read = unpack(&pack("two", &theme).unwrap()).unwrap();
        assert_eq!(
            read.table.pictures,
            vec!["pictures/pic.jpg", "pictures/pic-2.jpg"]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picture_that_is_missing_or_not_a_picture_is_not_exported() {
        let dir = scratch("export-refusals");
        let missing = format!("{}/gone.jpg", dir.display());
        let theme: OwnTheme = toml::from_str(&format!("pictures = [{missing:?}]\n")).unwrap();
        assert!(
            format!("{:#}", pack("mine", &theme).unwrap_err()).contains("there is no picture at")
        );
        fs::write(dir.join("text.jpg"), b"hello").unwrap();
        let text_theme: OwnTheme = toml::from_str(&format!(
            "pictures = [{:?}]\n",
            dir.join("text.jpg").display().to_string()
        ))
        .unwrap();
        assert!(
            format!("{:#}", pack("mine", &text_theme).unwrap_err()).contains("not a JPEG or PNG")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_tar_archive_is_refused() {
        assert!(refused(b"").contains("not a theme file"));
        assert!(refused(&[b'x'; 1000]).contains("not a theme file"));
        let mut two = archive(&[
            (THEME_FILE, b'0', theme_toml_text("mine", "").as_bytes()),
            ("pictures/a.jpg", b'0', &jpeg(2)),
        ]);
        // The second header's name is damaged, so its checksum is wrong.
        two[BLOCK + BLOCK + 3] = b'Z';
        assert!(refused(&two).contains("damaged header"));
    }

    #[test]
    fn a_file_without_theme_toml_first_is_refused() {
        let none = archive(&[("pictures/x.jpg", b'0', &jpeg(4))]);
        assert!(refused(&none).contains("no theme.toml"));
        let late = archive(&[
            ("pictures/x.jpg", b'0', &jpeg(4)),
            (
                THEME_FILE,
                b'0',
                theme_toml_text("mine", "\"pictures/x.jpg\"").as_bytes(),
            ),
        ]);
        assert!(refused(&late).contains("not the first entry"));
    }

    #[test]
    fn a_link_a_device_and_a_folder_are_refused_by_name() {
        let theme = theme_toml_text("mine", "").into_bytes();
        let link = archive(&[(THEME_FILE, b'0', &theme), ("pictures/link.jpg", b'2', b"")]);
        assert!(refused(&link).contains("pictures/link.jpg is a link"));
        let device = archive(&[(THEME_FILE, b'0', &theme), ("sda", b'4', b"")]);
        assert!(refused(&device).contains("sda is a device or a pipe"));
        let folder = archive(&[(THEME_FILE, b'0', &theme), ("docs/", b'5', b"")]);
        assert!(refused(&folder).contains("docs/ is a folder"));
    }

    #[test]
    fn an_entry_outside_theme_toml_and_pictures_is_refused_by_name() {
        let theme = theme_toml_text("mine", "").into_bytes();
        let runs = archive(&[(THEME_FILE, b'0', &theme), ("run.sh", b'0', b"echo hi\n")]);
        let message = refused(&runs);
        assert!(
            message.contains("run.sh is not part of a theme"),
            "{message}"
        );
    }

    #[test]
    fn an_absolute_path_and_a_dotdot_path_are_refused() {
        let theme = theme_toml_text("mine", "").into_bytes();
        let absolute = archive(&[(THEME_FILE, b'0', &theme), ("/etc/passwd", b'0', b"x")]);
        assert!(refused(&absolute).contains("/etc/passwd is an absolute path"));
        let dotdot = archive(&[
            (THEME_FILE, b'0', &theme),
            ("pictures/../x.jpg", b'0', &jpeg(2)),
        ]);
        assert!(refused(&dotdot).contains(".. in its path"));
    }

    #[test]
    fn a_name_outside_the_plain_characters_is_refused() {
        let theme = theme_toml_text("mine", "").into_bytes();
        let spaced = archive(&[
            (THEME_FILE, b'0', &theme),
            ("pictures/my pic.jpg", b'0', &jpeg(2)),
        ]);
        let message = refused(&spaced);
        assert!(message.contains("my pic.jpg"), "{message}");
        assert!(message.contains("file name"), "{message}");
    }

    #[test]
    fn a_duplicate_entry_is_refused() {
        let theme = theme_toml_text("mine", "").into_bytes();
        let twice = archive(&[
            (THEME_FILE, b'0', &theme),
            ("pictures/a.jpg", b'0', &jpeg(2)),
            ("pictures/a.jpg", b'0', &jpeg(2)),
        ]);
        assert!(refused(&twice).contains("pictures/a.jpg is in the file twice"));
    }

    #[test]
    fn a_picture_that_is_not_jpeg_or_png_is_refused() {
        let theme = theme_toml_text("mine", "\"pictures/a.gif\"").into_bytes();
        let gif = archive(&[
            (THEME_FILE, b'0', &theme),
            ("pictures/a.gif", b'0', b"GIF89a"),
        ]);
        assert!(refused(&gif).contains("pictures/a.gif is not a JPEG or PNG picture"));
    }

    #[test]
    fn a_picture_over_the_size_limit_is_refused() {
        let theme = theme_toml_text("mine", "\"pictures/big.jpg\"").into_bytes();
        let mut big = jpeg(0);
        big.resize(MOST_PICTURE_BYTES as usize + 1, 0);
        let bytes = archive(&[(THEME_FILE, b'0', &theme), ("pictures/big.jpg", b'0', &big)]);
        assert!(refused(&bytes).contains("pictures/big.jpg is over 32 MiB"));
    }

    #[test]
    fn a_reference_with_no_entry_and_an_entry_no_reference_uses_are_refused() {
        let missing = archive(&[(
            THEME_FILE,
            b'0',
            theme_toml_text("mine", "\"pictures/a.jpg\"").as_bytes(),
        )]);
        assert!(refused(&missing).contains("pictures/a.jpg, which the file does not hold"));
        let unused = archive(&[
            (THEME_FILE, b'0', theme_toml_text("mine", "").as_bytes()),
            ("pictures/a.jpg", b'0', &jpeg(2)),
        ]);
        assert!(refused(&unused).contains("pictures/a.jpg is in the file but no theme names it"));
    }

    #[test]
    fn a_reference_to_a_full_path_inside_the_file_is_refused() {
        let theme = theme_toml_text("mine", "\"/home/someone/x.jpg\"").into_bytes();
        let bytes = archive(&[(THEME_FILE, b'0', &theme)]);
        assert!(refused(&bytes).contains("a picture's full path"));
    }

    #[test]
    fn a_theme_the_checker_refuses_is_refused_with_its_problem() {
        let wrong = archive(&[(
            THEME_FILE,
            b'0',
            b"format = 1\nname = \"mine\"\nradius = \"wide\"\n",
        )]);
        let message = refused(&wrong);
        assert!(
            message.contains("theme.toml is not a theme this release reads"),
            "{message}"
        );
        let unknown = archive(&[(
            THEME_FILE,
            b'0',
            b"format = 1\nname = \"mine\"\nnot_a_key = 3\n",
        )]);
        assert!(
            refused(&unknown).contains("not_a_key")
                || refused(&unknown).contains("not a theme this release reads")
        );
        let no_format = archive(&[(THEME_FILE, b'0', b"name = \"mine\"\n")]);
        assert!(refused(&no_format).contains("no format = 1"));
        let bad_name = archive(&[(THEME_FILE, b'0', b"format = 1\nname = \"my theme\"\n")]);
        assert!(refused(&bad_name).contains("is not a theme's name"));
    }

    #[test]
    fn data_after_the_end_of_the_archive_is_refused() {
        let theme = theme_toml_text("mine", "").into_bytes();
        let mut bytes = archive(&[(THEME_FILE, b'0', &theme)]);
        bytes.extend_from_slice(b"junk");
        assert!(refused(&bytes).contains("data after its end"));
    }

    #[test]
    fn a_cut_short_archive_is_refused() {
        let theme = theme_toml_text("mine", "").into_bytes();
        let bytes = archive(&[(THEME_FILE, b'0', &theme)]);
        assert!(refused(&bytes[..bytes.len() - 2 * BLOCK]).contains("cut short"));
        assert!(refused(&bytes[..BLOCK + 3]).contains("cut short"));
    }

    #[test]
    fn a_name_clash_takes_the_next_free_name() {
        let taken = |name: &str| name == "mine";
        assert_eq!(themes::import_name("mine", taken), "mine-2");
        assert_eq!(themes::import_name("mine", |_| false), "mine");
        assert_eq!(themes::import_name("edel", |_| false), "edel-2");
    }

    #[test]
    fn the_archive_is_plain_ustar_that_tar_reads() {
        let dir = scratch("tar");
        let theme = own_theme(&dir);
        let file = dir.join("mine.edeltheme");
        fs::write(&file, pack("mine", &theme).unwrap()).unwrap();
        if let Ok(out) = std::process::Command::new("tar")
            .arg("tf")
            .arg(&file)
            .output()
        {
            let listed = String::from_utf8_lossy(&out.stdout).to_string();
            assert!(out.status.success(), "{listed}");
            assert!(listed.starts_with("theme.toml"), "{listed}");
            assert!(listed.contains("pictures/hills-day.jpg"));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_octal_size_is_refused() {
        assert_eq!(octal(b"0000017\0"), Some(15));
        assert_eq!(octal(b"  17 \0"), Some(15));
        assert_eq!(octal(b"1x\0"), None);
        assert_eq!(octal(&[0x80, 0, 0]), None);
    }

    #[test]
    fn the_extension_is_edeltheme() {
        assert_eq!(EXTENSION, "edeltheme");
    }

    #[test]
    fn export_never_writes_over_a_file_and_leaves_no_new_file_behind() {
        let dir = scratch("export-over");
        let target = dir.join("mine.edeltheme");
        fs::write(&target, b"keep").unwrap();
        let theme = own_theme(&dir);
        let message = format!("{:#}", export("mine", &theme, &target).unwrap_err());
        assert!(
            message.contains("is there already; give another file name"),
            "{message}"
        );
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        fs::remove_file(&target).unwrap();
        export("mine", &theme, &target).unwrap();
        assert_eq!(read(&target).unwrap().name, "mine");
        assert!(!dir.join("mine.edeltheme.new").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plan_names_the_theme_and_is_checked_against_the_file() {
        let dir = scratch("plan");
        let theme = unpack(&pack("mine", &own_theme(&dir)).unwrap()).unwrap();
        let fresh = plan(theme.clone(), "format = 1\n", &dir).unwrap();
        assert_eq!(fresh.name, "mine");
        assert_eq!(fresh.title, "Mine");
        assert_eq!(fresh.pictures, 2);
        assert!(fresh.keys.contains(&"appearance.accent".to_string()));
        let taken = plan(theme, &fresh.add("format = 1\n").unwrap(), &dir).unwrap();
        assert_eq!(taken.name, "mine-2");
        let _ = fs::remove_dir_all(&dir);
    }
}
