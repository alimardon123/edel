//! The people who have an account on this machine (roadmap M5.8b), read
//! from `/etc/passwd` and `/etc/group` for Settings' Users page: the
//! login name, the full name the account was given, whether the person is
//! an administrator, the home folder and the login shell. Nothing here
//! reads or writes a password; `/etc/shadow` is never opened. Adding,
//! removing and changing people is `edel settings` (`users.NAME.admin`,
//! `users.NAME.login_shell`, `users.NAME.ssh_keys`, applied as root), not
//! this module's. Std only.

use std::fs;

/// The group an administrator belongs to (`users.NAME.admin`, ADR-008),
/// the one place the name is written; `edel settings apply` adds and
/// removes members.
pub const ADMIN_GROUP: &str = "admin";

/// The account database, as the C library reads it.
const PASSWD: &str = "/etc/passwd";
const GROUP: &str = "/etc/group";

/// The first user id of a person: below it are the system's own accounts.
const FIRST_UID: u32 = 1000;

/// The user id `nobody` has, which no person has or goes above.
const NOBODY: u32 = 65534;

/// One person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    /// The name they log in with
    pub name: String,
    /// The name the account was given, as people say it; empty when it
    /// was given none
    pub full_name: String,
    pub uid: u32,
    pub home: String,
    pub shell: String,
    /// Whether they belong to [`ADMIN_GROUP`]
    pub admin: bool,
}

impl Person {
    /// What to call them: their full name, else the name they log in with.
    pub fn shown(&self) -> &str {
        if self.full_name.is_empty() {
            &self.name
        } else {
            &self.full_name
        }
    }
}

/// Whether `shell` is one a person can log in with: not empty, and not
/// the programs that refuse a login.
fn logs_in(shell: &str) -> bool {
    let program = shell.rsplit('/').next().unwrap_or_default();
    !program.is_empty() && program != "nologin" && program != "false"
}

/// The people in `passwd` (the text of `/etc/passwd`), `group` being
/// `/etc/group`'s: those with a user id from 1000 below `nobody`'s and a
/// shell they can log in with, in the order the file has them, and
/// `current` (a user id), root or a stick's `live` account included, first.
pub fn people(passwd: &str, group: &str, current: Option<u32>) -> Vec<Person> {
    let admins: Vec<&str> = group
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            (f.len() == 4 && f[0] == ADMIN_GROUP).then_some(f[3])
        })
        .flat_map(|members| members.split(','))
        .filter(|m| !m.is_empty())
        .collect();
    let mut all: Vec<Person> = passwd
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() != 7 {
                return None;
            }
            let uid: u32 = f[2].parse().ok()?;
            let is_current = current == Some(uid);
            let a_person = (FIRST_UID..NOBODY).contains(&uid) && logs_in(f[6]);
            (a_person || is_current).then(|| Person {
                name: f[0].to_string(),
                // The GECOS field is `Full name,Room,Phone,...`.
                full_name: f[4]
                    .split(',')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                uid,
                home: f[5].to_string(),
                shell: f[6].to_string(),
                // Root can do everything, group or not.
                admin: uid == 0 || admins.contains(&f[0]),
            })
        })
        .collect();
    if let Some(at) = all.iter().position(|p| Some(p.uid) == current) {
        let you = all.remove(at);
        all.insert(0, you);
    }
    all
}

/// The user id of the person running this program: the owner of its own
/// `/proc` entry, which needs no C library call.
pub fn current() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// The people on this machine now, `current` first.
pub fn read(current: Option<u32>) -> Vec<Person> {
    let read = |path: &str| fs::read_to_string(path).unwrap_or_default();
    people(&read(PASSWD), &read(GROUP), current)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD_TEXT: &str = "\
root:x:0:0:root:/root:/bin/ash
daemon:x:2:2:daemon:/sbin:/sbin/nologin
greetd:x:101:101:greetd:/var/lib/greetd:/sbin/nologin
live:x:102:102:Live session:/home/live:/bin/sh
ali:x:1000:1000:Ali Karimov,,,:/home/ali:/bin/ash
bob:x:1001:1001::/home/bob:/bin/sh
guest:x:1002:1002:Guest:/home/guest:/sbin/nologin
nobody:x:65534:65534:nobody:/:/sbin/nologin
broken line
";
    const GROUP_TEXT: &str = "root:x:0:root\nadmin:x:300:ali\nseat:x:301:ali,bob\n";

    fn names(people: &[Person]) -> Vec<&str> {
        people.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn people_are_the_accounts_that_can_log_in_from_1000_up() {
        let people = people(PASSWD_TEXT, GROUP_TEXT, None);
        assert_eq!(names(&people), ["ali", "bob"], "no system accounts");
        assert_eq!(people[0].shown(), "Ali Karimov", "the first GECOS field");
        assert_eq!(
            people[1].shown(),
            "bob",
            "the login name without a full name"
        );
        assert!(people[0].admin && !people[1].admin);
        assert_eq!(people[0].home, "/home/ali");
        assert_eq!(people[0].shell, "/bin/ash");
    }

    #[test]
    fn the_person_at_the_machine_comes_first_whoever_they_are() {
        let by_uid = |uid| people(PASSWD_TEXT, GROUP_TEXT, Some(uid));
        assert_eq!(names(&by_uid(1001)), ["bob", "ali"]);
        // A stick's live account is a system account, and is the person.
        assert_eq!(names(&by_uid(102)), ["live", "ali", "bob"]);
        assert_eq!(names(&by_uid(0)), ["root", "ali", "bob"]);
        assert!(by_uid(0)[0].admin, "root can do everything");
        // A user id the file lacks adds nobody.
        assert_eq!(names(&by_uid(4242)), ["ali", "bob"]);
    }

    #[test]
    fn a_missing_group_means_nobody_is_an_administrator() {
        let people = people(PASSWD_TEXT, "root:x:0:root\n", None);
        assert!(people.iter().all(|p| !p.admin));
        assert!(super::people("", "", None).is_empty());
    }

    #[test]
    fn a_shell_that_refuses_a_login_is_not_one_to_log_in_with() {
        assert!(logs_in("/bin/ash") && logs_in("/usr/bin/fish"));
        assert!(!logs_in("/sbin/nologin") && !logs_in("/bin/false") && !logs_in(""));
    }
}
