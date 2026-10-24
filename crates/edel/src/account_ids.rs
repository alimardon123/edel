//! The system accounts' pinned ids (`features/base.toml`'s `[alpine.users]`
//! and `[alpine.groups]`). busybox's `adduser -S` and `addgroup -S` hand
//! out the first free id from 100 in the order apk installs packages, so
//! one package more moves the others. After every `apk add`, the image
//! builder moves each system account to its pinned id here, and the files
//! that carry the old id with it, and fails on a system account that has
//! no pin. A newer slot's new account then never takes an id that a
//! machine gave another account, which `data.rs`'s merge would have to
//! leave out, and an add-on's accounts are the same on every build.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::ops::RangeInclusive;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use anyhow::{Context, Result, bail};

/// The ids busybox hands system accounts.
const SYSTEM: RangeInclusive<u32> = 100..=999;

/// The ids of Edel OS's own system accounts, which `edel` makes on the
/// machine (the stick's live person and the admin group): no pin may be
/// one of them.
const OWN: [(&str, &str, u32); 2] = [
    ("user", edel::places::LIVE_USER, edel::places::LIVE_UID),
    ("group", edel::users::ADMIN_GROUP, edel::users::ADMIN_GID),
];

/// Old ids to new ones, for users and for groups.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Moves {
    pub users: BTreeMap<u32, u32>,
    pub groups: BTreeMap<u32, u32>,
}

impl Moves {
    fn is_empty(&self) -> bool {
        self.users.is_empty() && self.groups.is_empty()
    }
}

/// Checks the pins themselves: each in the system range, no two names of
/// one kind on one id, none on an id of Edel OS's own accounts.
pub fn check(users: &BTreeMap<String, u32>, groups: &BTreeMap<String, u32>) -> Result<()> {
    for (kind, pins) in [("user", users), ("group", groups)] {
        let mut seen: BTreeMap<u32, &str> = BTreeMap::new();
        for (name, id) in pins {
            if !SYSTEM.contains(id) {
                bail!(
                    "alpine.{kind}s.{name} is {id}; a system {kind}'s id is from {} to {}",
                    SYSTEM.start(),
                    SYSTEM.end()
                );
            }
            if let Some(other) = seen.insert(*id, name) {
                bail!("alpine.{kind}s gives {other} and {name} the same id, {id}");
            }
            if let Some((_, own, _)) = OWN.iter().find(|(k, _, i)| *k == kind && i == id) {
                bail!("alpine.{kind}s.{name} is {id}, the id of Edel OS's own {kind} {own}");
            }
        }
    }
    Ok(())
}

/// The name and id of each account line of `text` (`passwd` has 7
/// fields, `group` 4; the id is the third).
fn ids(text: &str) -> impl Iterator<Item = (&str, u32)> {
    text.lines().filter_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        matches!(fields.len(), 4 | 7)
            .then(|| fields[2].parse().ok().map(|id| (fields[0], id)))
            .flatten()
    })
}

/// What moves in one account file: every system account to its pin.
/// Fails on a system account without one, and when two names would end
/// on one id.
fn moves_of(kind: &str, text: &str, pins: &BTreeMap<String, u32>) -> Result<BTreeMap<u32, u32>> {
    let mut moves = BTreeMap::new();
    let mut after: BTreeMap<u32, &str> = BTreeMap::new();
    for (name, id) in ids(text) {
        let new = match pins.get(name) {
            Some(pin) => *pin,
            None if SYSTEM.contains(&id) => bail!(
                "a package added the system {kind} {name} (id {id}), whose id features/base.toml does not pin; add `{name} = N` to its [alpine.{kind}s], N the lowest id from 900 up that the table does not use"
            ),
            None => id,
        };
        if new != id {
            moves.insert(id, new);
        }
        if let Some(other) = after.insert(new, name) {
            bail!("the {kind}s {other} and {name} would both have the id {new}");
        }
    }
    Ok(moves)
}

/// The account file `text` with the id in field `field` of each line
/// moved by `moves`.
fn rewrite(text: &str, field: usize, moves: &BTreeMap<u32, u32>) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (body, end) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        let mut fields: Vec<String> = body.split(':').map(String::from).collect();
        if matches!(fields.len(), 4 | 7)
            && let Some(new) = fields[field]
                .parse()
                .ok()
                .and_then(|id: u32| moves.get(&id))
        {
            fields[field] = new.to_string();
        }
        out.push_str(&fields.join(":"));
        out.push_str(end);
    }
    out
}

/// The moves for a root's `passwd` and `group`, and the files as they
/// become.
fn plan(
    passwd: &str,
    group: &str,
    users: &BTreeMap<String, u32>,
    groups: &BTreeMap<String, u32>,
) -> Result<(Moves, String, String)> {
    let moves = Moves {
        users: moves_of("user", passwd, users)?,
        groups: moves_of("group", group, groups)?,
    };
    let passwd = rewrite(&rewrite(passwd, 2, &moves.users), 3, &moves.groups);
    let group = rewrite(group, 2, &moves.groups);
    Ok((moves, passwd, group))
}

/// Moves the system accounts of the root at `root` to their pins, and the
/// files that carry an old id with them. A root without account files
/// yet (no base layout) is left alone.
pub fn pin(
    root: &Path,
    users: &BTreeMap<String, u32>,
    groups: &BTreeMap<String, u32>,
) -> Result<()> {
    let (passwd_at, group_at) = (root.join("etc/passwd"), root.join("etc/group"));
    let (Ok(passwd), Ok(group)) = (
        fs::read_to_string(&passwd_at),
        fs::read_to_string(&group_at),
    ) else {
        return Ok(());
    };
    let (moves, new_passwd, new_group) = plan(&passwd, &group, users, groups)?;
    if moves.is_empty() {
        return Ok(());
    }
    println!(
        "moving system accounts to their pinned ids: users {:?}, groups {:?}",
        moves.users, moves.groups
    );
    fs::write(&passwd_at, new_passwd)?;
    fs::write(&group_at, new_group)?;
    let device = fs::metadata(root)?.dev();
    let mut seen = HashSet::new();
    chown_tree(root, device, &moves, &mut seen)
}

/// Gives every file under `dir` on the root's own file system (not the
/// proc, sys and dev mounted for the package scripts) the new ids. A
/// change of owner drops the set-id bits and the file's capabilities, so
/// the mode and the attributes are put back.
fn chown_tree(dir: &Path, device: u64, moves: &Moves, seen: &mut HashSet<u64>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.dev() != device {
            continue;
        }
        let uid = moves.users.get(&meta.uid()).copied();
        let gid = moves.groups.get(&meta.gid()).copied();
        if (uid.is_some() || gid.is_some()) && seen.insert(meta.ino()) {
            let attrs = crate::xattrs::read(&path)?;
            std::os::unix::fs::lchown(&path, uid, gid)
                .with_context(|| format!("changing the owner of {}", path.display()))?;
            if !meta.file_type().is_symlink() {
                fs::set_permissions(&path, fs::Permissions::from_mode(meta.mode() & 0o7777))?;
            }
            crate::xattrs::write(&path, &attrs)?;
        }
        if meta.is_dir() {
            chown_tree(&path, device, moves, seen)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins(list: &[(&str, u32)]) -> BTreeMap<String, u32> {
        list.iter().map(|(n, i)| (n.to_string(), *i)).collect()
    }

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/sh\nmessagebus:x:100:101:messagebus:/dev/null:/sbin/nologin\ngreetd:x:101:102:greetd:/var/lib/greetd:/sbin/nologin\nnobody:x:65534:65534:nobody:/:/sbin/nologin\n";
    const GROUP: &str =
        "root:x:0:root\nvideo:x:27:root,greetd\nmessagebus:x:101:messagebus\ngreetd:x:102:greetd\n";

    #[test]
    fn system_accounts_move_to_their_pins_and_others_stay() {
        let users = pins(&[("messagebus", 101), ("greetd", 104)]);
        let groups = pins(&[("messagebus", 102), ("greetd", 107)]);
        let (moves, passwd, group) = plan(PASSWD, GROUP, &users, &groups).unwrap();
        assert_eq!(moves.users, BTreeMap::from([(100, 101), (101, 104)]));
        assert_eq!(moves.groups, BTreeMap::from([(101, 102), (102, 107)]));
        assert_eq!(
            passwd,
            "root:x:0:0:root:/root:/bin/sh\nmessagebus:x:101:102:messagebus:/dev/null:/sbin/nologin\ngreetd:x:104:107:greetd:/var/lib/greetd:/sbin/nologin\nnobody:x:65534:65534:nobody:/:/sbin/nologin\n"
        );
        assert_eq!(
            group,
            "root:x:0:root\nvideo:x:27:root,greetd\nmessagebus:x:102:messagebus\ngreetd:x:107:greetd\n"
        );
        let (moves, ..) = plan(&passwd, &group, &users, &groups).unwrap();
        assert_eq!(moves, Moves::default());
    }

    #[test]
    fn an_unpinned_system_account_or_two_on_one_id_fails() {
        let users = pins(&[("messagebus", 100)]);
        let groups = pins(&[("messagebus", 101), ("greetd", 102)]);
        let err = plan(PASSWD, GROUP, &users, &groups)
            .unwrap_err()
            .to_string();
        assert!(err.contains("system user greetd (id 101)"), "{err}");
        assert!(err.contains("[alpine.users]"), "{err}");
        let users = pins(&[("messagebus", 100), ("greetd", 100)]);
        let err = plan(PASSWD, GROUP, &users, &groups)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("messagebus and greetd would both have the id 100"),
            "{err}"
        );
    }

    #[test]
    fn pins_stay_in_the_system_range_apart_and_off_our_own_ids() {
        assert!(check(&pins(&[("a", 100), ("b", 999)]), &pins(&[("a", 100)])).is_ok());
        assert!(check(&pins(&[("a", 1000)]), &BTreeMap::new()).is_err());
        assert!(check(&pins(&[("a", 120), ("b", 120)]), &BTreeMap::new()).is_err());
        let err = check(&BTreeMap::new(), &pins(&[("a", edel::users::ADMIN_GID)]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("own group admin"), "{err}");
    }

    #[test]
    fn the_base_pins_pass_their_own_check() {
        let text = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features/base.toml"),
        )
        .unwrap();
        let (feature, notes) = edel::features::read(&text).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        let alpine = feature.alpine.unwrap();
        check(&alpine.users, &alpine.groups).unwrap();
        assert!(alpine.users.contains_key("greetd") && alpine.groups.contains_key("seat"));
    }
}
