//! Fleets (M7.3, docs/FLEET.md): settings files merged into a fleet image's
//! own, and the stick that installs itself.

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, TableLike};

use crate::i18n::{tr, trf};
use crate::settings;

/// Merges settings files in order into one text: a later file wins where
/// two set the same key, a table (standard or inline) is merged key by
/// key, and any other value, a list included, is replaced whole. The
/// first file's comments and order stay. `named` gives each text with the
/// words a message shows for it (its path), so a file that is not TOML is
/// named.
pub fn merge(named: &[(String, String)]) -> Result<String> {
    let Some(((first_name, first), rest)) = named.split_first() else {
        bail!("{}", tr("no settings file to merge"));
    };
    let mut base = parse(first_name, first)?;
    for (name, text) in rest {
        let over = parse(name, text)?;
        merge_into(base.as_table_mut(), over.as_table())?;
    }
    Ok(base.to_string())
}

/// Parses one settings text, naming the file when it is not TOML; the
/// parser's line and column stay in the chain.
fn parse(name: &str, text: &str) -> Result<DocumentMut> {
    text.parse::<DocumentMut>()
        .with_context(|| trf("{path} is not valid TOML", &[("path", name)]))
}

/// Copies every key of `over` into `base`, a table at a time: a key both
/// set as tables (standard or inline, in either order) merges, anything
/// else is replaced by `over`'s item.
fn merge_into(base: &mut dyn TableLike, over: &dyn TableLike) -> Result<()> {
    for (key, item) in over.iter() {
        let merged = match (
            base.get_mut(key).and_then(|b| b.as_table_like_mut()),
            item.as_table_like(),
        ) {
            (Some(b), Some(o)) => {
                merge_into(b, o)?;
                true
            }
            _ => false,
        };
        if !merged {
            // The comment lines above a key belong to the key, so a value
            // replaced whole keeps the first file's lines above it. An
            // inline table base converts a standard table item into an
            // inline one itself (toml_edit's TableLike insert); `iter`
            // skips items that are none, so every item here converts.
            let decor = base.key(key).map(|k| k.leaf_decor().clone());
            base.insert(key, item.clone());
            if let (Some(decor), Some(mut k)) = (decor, base.key_mut(key)) {
                *k.leaf_decor_mut() = decor;
            }
        }
    }
    Ok(())
}

/// What `[install]` asks for, with its defaults filled in (docs/FLEET.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unattended {
    /// `None` for `auto`, else the name under `/dev/disk/by-id/`
    pub by_id: Option<String>,
    /// Whether any disk may be erased (`erase = "yes"`), not only an empty one
    pub erase_any: bool,
    /// Whether the machine restarts afterwards, rather than powering off
    pub reboot: bool,
}

/// The `[install]` table of a settings file's text, when it has one;
/// `edel settings check`'s rules for it must hold, so a file that breaks
/// them is refused here too, before any disk is looked at.
pub fn unattended(text: &str) -> Result<Option<Unattended>> {
    let doc: DocumentMut = text
        .parse()
        .with_context(|| tr("the settings file is not valid TOML"))?;
    if !doc.contains_key("install") {
        return Ok(None);
    }
    let read = settings::read(text)?;
    let problems: Vec<String> = settings::check(text)?
        .into_iter()
        .filter(|line| line.starts_with("install."))
        .collect();
    if !problems.is_empty() {
        bail!(
            "{}\n{}",
            tr("install refused: the [install] table has problems; fix these:"),
            problems.join("\n")
        );
    }
    let install = read.file.install;
    let by_id = install
        .disk
        .as_deref()
        .and_then(settings::by_id_name)
        .map(str::to_string);
    Ok(Some(Unattended {
        erase_any: by_id.is_some() && install.erase.as_deref() == Some("yes"),
        reboot: install.then.as_deref() == Some("reboot"),
        by_id,
    }))
}

/// The settings file without its `[install]` table, as the installed
/// machine gets it: only `edel install` reads that table (ADR-008).
pub fn without_install(text: &str) -> Result<String> {
    let mut doc: DocumentMut = text
        .parse()
        .with_context(|| tr("the settings file is not valid TOML"))?;
    doc.remove("install");
    Ok(doc.to_string())
}

/// Whether a machine made from this file lets somebody log in with nobody
/// at its keyboard: it names nobody, so the first start asks for a person
/// (the console's question, or the Welcome), or one of the people it names
/// has an ssh key. People with no key have no password either, as no
/// password is ever in a settings file.
pub fn someone_can_log_in(file: &settings::SettingsFile) -> bool {
    file.users.is_empty()
        || file
            .users
            .values()
            .any(|user| user.ssh_keys.as_ref().is_some_and(|keys| !keys.is_empty()))
}

/// Why a file with `[install]` that leaves nobody able to log in is
/// refused, by the image build and by the install alike.
pub fn nobody_can_log_in() -> &'static str {
    tr(
        "the file names people and none of them has an ssh key, so nobody could log in to a machine installed with nobody at its keyboard; give one of them ssh_keys",
    )
}

/// A disk `auto` may pick: its name, whether it is removable, its size and
/// its id under `/dev/disk/by-id/`, if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoDisk {
    pub name: String,
    pub removable: bool,
    pub bytes: u64,
    pub id: Option<String>,
}

/// The one disk `install.disk = "auto"` picks: of the disks `edel install
/// --list` offers but the running one, those that are not removable and
/// have at least `need` bytes. None, or more than one, is refused, so an
/// unattended install never guesses between disks.
pub fn pick_auto(disks: &[AutoDisk], running: &str, need: u64) -> Result<AutoDisk> {
    let fit: Vec<&AutoDisk> = disks
        .iter()
        .filter(|d| d.name != running && !d.removable && d.bytes >= need)
        .collect();
    match fit.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!(
            "{}",
            trf(
                "install refused: no disk to install on; auto takes a disk that is not removable, not the one this system runs from, and at least {size}",
                &[("size", &crate::install::size(need))]
            )
        ),
        many => bail!(
            "{}",
            trf(
                "install refused: auto found {count} disks it could take ({names}); name one in install.disk by its /dev/disk/by-id/ name",
                &[
                    ("count", &many.len().to_string()),
                    (
                        "names",
                        &many
                            .iter()
                            .map(|d| format!("/dev/{}", d.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                ]
            )
        ),
    }
}

/// The id of the disk `name` under `/dev/disk/by-id/`, from `links`, each
/// a link's name and the device it points at (`virtio-abc`, `vdb`): the
/// first by name that is the whole disk, never a partition's.
pub fn by_id_of(links: &[(String, String)], name: &str) -> Option<String> {
    let mut ids: Vec<&String> = links
        .iter()
        .filter(|(id, target)| target == name && !id.contains("-part"))
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    ids.first().map(|id| id.to_string())
}

/// Whether the first bytes of a disk hold nothing: an empty disk is all
/// zeros there, where any partition table or file system would be.
pub fn is_blank(head: &[u8]) -> bool {
    head.iter().all(|b| *b == 0)
}

/// The text of [`crate::places::INSTALLED_TO`], which a stick keeps once it
/// installed itself: the disk's id and the date.
pub fn installed_to_text(id: &str, date: &str) -> String {
    let mut doc = DocumentMut::new();
    doc["format"] = toml_edit::value(1);
    doc["disk"] = toml_edit::value(id);
    doc["date"] = toml_edit::value(date);
    doc.to_string()
}

/// The disk id [`installed_to_text`] wrote, if the text holds one.
pub fn installed_to_id(text: &str) -> Option<String> {
    let doc: DocumentMut = text.parse().ok()?;
    doc.get("disk")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(name: &str, text: &str) -> (String, String) {
        (name.to_string(), text.to_string())
    }

    fn value(merged: &str) -> toml::Table {
        toml::from_str(merged).expect("the merged text is TOML")
    }

    #[test]
    fn a_later_file_wins_a_key() {
        let merged = merge(&[
            text("a.toml", "format = 1\n[network]\nhostname = \"a\"\n"),
            text("b.toml", "[network]\nhostname = \"b\"\n"),
        ])
        .expect("both files merge");
        assert_eq!(value(&merged)["network"]["hostname"].as_str(), Some("b"));
    }

    #[test]
    fn tables_merge_key_by_key() {
        let merged = merge(&[
            text("a.toml", "[users.ali]\nadmin = true\n"),
            text(
                "b.toml",
                "[users.ali]\nssh_keys = [\"k\"]\n[users.bea]\nadmin = false\n",
            ),
        ])
        .expect("both files merge");
        let users = &value(&merged)["users"];
        assert_eq!(users["ali"]["admin"].as_bool(), Some(true));
        assert_eq!(users["ali"]["ssh_keys"][0].as_str(), Some("k"));
        assert_eq!(users["bea"]["admin"].as_bool(), Some(false));
    }

    #[test]
    fn lists_are_replaced_whole() {
        let merged = merge(&[
            text("a.toml", "[apps]\ninstalled = [\"a\", \"b\"]\n"),
            text("b.toml", "[apps]\ninstalled = [\"c\"]\n"),
        ])
        .expect("both files merge");
        let list = value(&merged)["apps"]["installed"]
            .as_array()
            .expect("a list")
            .clone();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].as_str(), Some("c"));
    }

    #[test]
    fn an_inline_table_merges_with_a_standard_one() {
        let merged = merge(&[
            text("a.toml", "users = { ali = { admin = true } }\n"),
            text("b.toml", "[users.ali]\nlogin_shell = \"/bin/sh\"\n"),
        ])
        .expect("both files merge");
        let ali = &value(&merged)["users"]["ali"];
        assert_eq!(ali["admin"].as_bool(), Some(true));
        assert_eq!(ali["login_shell"].as_str(), Some("/bin/sh"));
    }

    #[test]
    fn the_first_files_comments_stay() {
        let merged = merge(&[
            text("a.toml", "# fleet\nformat = 1\n"),
            text("b.toml", "format = 1\n"),
        ])
        .expect("both files merge");
        assert!(merged.contains("# fleet"));
    }

    #[test]
    fn a_standard_table_merges_with_an_inline_one() {
        let merged = merge(&[
            text("a.toml", "[users.ali]\nadmin = true\n"),
            text("b.toml", "users = { bea = { admin = true } }\n"),
        ])
        .expect("both files merge");
        let users = &value(&merged)["users"];
        assert_eq!(users["ali"]["admin"].as_bool(), Some(true));
        assert_eq!(users["bea"]["admin"].as_bool(), Some(true));
    }

    #[test]
    fn a_file_that_is_not_toml_is_named() {
        let err = merge(&[
            text("a.toml", "format = 1\n"),
            text("broken.toml", "[network\nhostname = \"x\"\n"),
        ])
        .expect_err("a file that is not TOML is refused");
        assert!(format!("{err:#}").contains("broken.toml"));
    }

    #[test]
    fn nothing_to_merge_is_refused() {
        assert!(merge(&[]).is_err());
    }

    const KEY: &str = "ssh-ed25519 AAAA ci";

    #[test]
    fn a_file_without_install_asks_for_nothing() {
        assert_eq!(unattended("format = 1\n").unwrap(), None);
    }

    #[test]
    fn install_takes_its_defaults() {
        let asked = unattended("format = 1\n[install]\n").unwrap().unwrap();
        assert_eq!(
            asked,
            Unattended {
                by_id: None,
                erase_any: false,
                reboot: false
            }
        );
        let asked = unattended(
            "format = 1\n[install]\ndisk = \"/dev/disk/by-id/virtio-x\"\nerase = \"yes\"\nthen = \"reboot\"\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(asked.by_id.as_deref(), Some("virtio-x"));
        assert!(asked.erase_any && asked.reboot);
    }

    #[test]
    fn a_broken_install_table_is_refused() {
        let err = unattended("format = 1\n[install]\ndisk = \"/dev/sda\"\n").unwrap_err();
        assert!(format!("{err:#}").contains("install refused"), "{err:#}");
    }

    #[test]
    fn the_copy_drops_install_and_keeps_the_rest() {
        let text =
            "format = 1\n# the fleet\n[network]\nhostname = \"a\"\n\n[install]\ndisk = \"auto\"\n";
        let copy = without_install(text).unwrap();
        assert!(!copy.contains("install"), "{copy}");
        assert!(copy.contains("# the fleet") && copy.contains("hostname = \"a\""));
        let dotted = without_install("format = 1\ninstall.disk = \"auto\"\n").unwrap();
        assert!(!dotted.contains("install"), "{dotted}");
    }

    #[test]
    fn somebody_must_be_able_to_log_in() {
        let file = |text: &str| settings::read(text).unwrap().file;
        assert!(someone_can_log_in(&file("format = 1\n")));
        assert!(!someone_can_log_in(&file(
            "format = 1\n[users.ali]\nadmin = true\n"
        )));
        assert!(someone_can_log_in(&file(&format!(
            "format = 1\n[users.ali]\nadmin = true\n[users.bea]\nssh_keys = [\"{KEY}\"]\n"
        ))));
        assert!(!someone_can_log_in(&file(
            "format = 1\n[users.ali]\nssh_keys = []\n"
        )));
    }

    fn disk(name: &str, removable: bool, gib: u64) -> AutoDisk {
        AutoDisk {
            name: name.into(),
            removable,
            bytes: gib << 30,
            id: Some(format!("virtio-{name}")),
        }
    }

    #[test]
    fn auto_picks_the_one_fixed_disk_large_enough() {
        let disks = [
            disk("vda", false, 10),
            disk("vdb", false, 10),
            disk("sda", true, 32),
            disk("vdc", false, 1),
        ];
        assert_eq!(pick_auto(&disks, "vda", 9 << 30).unwrap().name, "vdb");
    }

    #[test]
    fn auto_never_guesses() {
        let two = [disk("vdb", false, 10), disk("vdc", false, 10)];
        let err = pick_auto(&two, "vda", 9 << 30).unwrap_err().to_string();
        assert!(err.contains("/dev/vdb, /dev/vdc"), "{err}");
        let none = [disk("vda", false, 10), disk("sda", true, 32)];
        assert!(pick_auto(&none, "vda", 9 << 30).is_err());
    }

    #[test]
    fn by_id_takes_the_whole_disk() {
        let links = [
            ("virtio-b-part1".to_string(), "vdb1".to_string()),
            ("virtio-b".to_string(), "vdb".to_string()),
            ("virtio-a".to_string(), "vda".to_string()),
        ];
        assert_eq!(by_id_of(&links, "vdb").as_deref(), Some("virtio-b"));
        assert_eq!(by_id_of(&links, "vdc"), None);
    }

    #[test]
    fn only_zeros_are_blank() {
        assert!(is_blank(&[0; 512]));
        let mut head = [0u8; 512];
        head[510] = 0x55;
        assert!(!is_blank(&head));
    }

    #[test]
    fn installed_to_reads_back() {
        let text = installed_to_text("virtio-fleet", "2026-10-17");
        assert_eq!(installed_to_id(&text).as_deref(), Some("virtio-fleet"));
        assert!(text.contains("date = \"2026-10-17\""));
        assert_eq!(installed_to_id("not toml ["), None);
    }
}
