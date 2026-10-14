//! Web apps (M6.4, part 1): a website made an app of its own, with its own
//! name and icon in the launcher and the window switcher, and its own
//! window. Firefox (`BROWSER`, the default browser of M6.2, a Flatpak app)
//! has no app mode on Linux, so each web app runs Firefox with a small
//! profile of its own, whose toolbars are hidden, and with its own Wayland
//! app id (`id_for`). What a web app is lives in two places only: its
//! desktop file in the person's applications folder, which carries
//! `X-Edel-Web-App` with its website, and its profile folder inside
//! Firefox's own Flatpak data, both named by `id_for`. `list` and `remove`
//! find only files that carry the marker, so nothing else is touched.
//!
//! The folders are read from `places` and passed on, so the tests use temp
//! folders and never the person's own.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::i18n::{tr, trf};
use crate::places;

/// The browser web apps run in: the default apps' browser (ADR-003, M6.2),
/// one Flathub app id. Its profiles are kept in its own Flatpak folder.
pub const BROWSER: &str = "org.mozilla.firefox";

/// The desktop file key that marks a web app as Edel OS's, and holds its
/// website. Only files carrying it are listed or removed.
pub const MARKER: &str = "X-Edel-Web-App";

/// The start of every web app's id.
const ID_PREFIX: &str = "edel-web-";

/// A web app that exists: its id (the desktop file's name and the window's
/// app id), its name in the launcher and its website.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebApp {
    pub id: String,
    pub name: String,
    pub url: String,
}

/// Whether a label of a host name is well formed: letters, digits and
/// hyphens, not empty and not starting or ending with a hyphen.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The host of an `http://` or `https://` address, or None when it has
/// none, without any user name, port or path.
fn host_of(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty() && host.split('.').all(is_label)).then_some(host)
}

/// The name a website gets when none is given: its host without `www.`,
/// as in `https://www.example.org/x` gives `example.org`. None for
/// anything that is not an `http://` or `https://` address with a host.
pub fn name_from_url(url: &str) -> Option<String> {
    let host = host_of(url)?.to_lowercase();
    let bare = host.strip_prefix("www.").filter(|rest| !rest.is_empty());
    Some(bare.unwrap_or(&host).to_string())
}

/// Whether `text` is a website typed as one: no spaces, and either an
/// `http://` or `https://` address, or `host.tld` with an optional path,
/// where the host holds letters, digits, dots and hyphens and a letter
/// follows its last dot. Such text is what the launcher offers to install.
pub fn is_url(text: &str) -> bool {
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return false;
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return name_from_url(text).is_some();
    }
    let host = text.split('/').next().unwrap_or("");
    let Some((_, tld)) = host.rsplit_once('.') else {
        return false;
    };
    host.split('.').all(is_label) && tld.starts_with(|c: char| c.is_ascii_alphabetic())
}

/// The address as a web app opens it: `text` as it is when it has a
/// scheme, else with `https://` in front.
pub fn normalise(text: &str) -> String {
    let text = text.trim();
    if text.starts_with("http://") || text.starts_with("https://") {
        text.to_string()
    } else {
        format!("https://{text}")
    }
}

/// The id of the web app called `name`: `edel-web-` and the name with
/// every character outside `a-z` and `0-9` turned to `-`, in lower case,
/// runs of `-` collapsed and none at the ends (`example.org` gives
/// `edel-web-example-org`). It is the desktop file's name and the window's
/// app id.
pub fn id_for(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("{ID_PREFIX}{slug}")
}

/// A value an `Exec` line carries as one word: as it is when only safe
/// characters are in it (none the desktop entry spec reserves), else in double quotes with `"`, `` ` ``, `$` and
/// `\` escaped.
fn exec_word(value: &str) -> String {
    let safe = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:=%+,@".contains(c));
    if safe {
        return value.to_string();
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        if "\"`$\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// `url` with the four characters that would need a backslash inside an
/// `Exec` line's quotes (`"`, `` ` ``, `$`, `\`) percent-encoded, as a
/// browser reads them alike, so no reader's escaping rules matter.
fn url_for_exec(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            '"' => out.push_str("%22"),
            '`' => out.push_str("%60"),
            '$' => out.push_str("%24"),
            '\\' => out.push_str("%5C"),
            c => out.push(c),
        }
    }
    out
}

/// The desktop entry of a web app: its name, its website as the comment,
/// the command that runs Firefox with the app's own profile and id, the
/// id as the window class so the window's app id matches the file, a
/// generic icon until favicons come, and the marker `X-Edel-Web-App`.
/// The website is quoted as the profile is when it holds more than safe
/// characters, and its `%` is written `%%`, as the desktop entry spec
/// asks of `Exec`. No branch or architecture is named, so the one file
/// serves x86_64 and arm64 alike.
pub fn desktop_text(name: &str, url: &str, id: &str, profile: &Path) -> String {
    let profile = exec_word(&profile.display().to_string());
    let exec_url = exec_word(&url_for_exec(url)).replace('%', "%%");
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment={url}\n\
         Exec=flatpak run {BROWSER} --new-instance --no-remote --name {id} --profile {profile} {exec_url}\n\
         StartupWMClass={id}\n\
         Icon=web-browser\n\
         Categories=Network;WebBrowser;\n\
         {MARKER}={url}\n"
    )
}

/// The `user.js` of a web app's profile: Firefox's own settings written at
/// each start, which let its `userChrome.css` hide the toolbars and keep
/// first-run and default-browser questions away. A web app is a website
/// in a window of its own, so it never shows Firefox's own welcome, its
/// terms of use box or its data notice: the person met those in Firefox
/// itself, and a new web app is a new profile that would ask again.
pub fn profile_prefs() -> &'static str {
    "user_pref(\"toolkit.legacyUserProfileCustomizations.stylesheets\", true);\n\
     user_pref(\"browser.shell.checkDefaultBrowser\", false);\n\
     user_pref(\"browser.tabs.inTitlebar\", 0);\n\
     user_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");\n\
     user_pref(\"datareporting.policy.dataSubmissionEnabled\", false);\n\
     user_pref(\"datareporting.policy.dataSubmissionPolicyBypassNotification\", true);\n\
     user_pref(\"toolkit.telemetry.reportingpolicy.firstRun\", false);\n\
     user_pref(\"browser.aboutwelcome.enabled\", false);\n\
     user_pref(\"trailhead.firstrun.didSeeAboutWelcome\", true);\n\
     user_pref(\"browser.preonboarding.enabled\", false);\n\
     user_pref(\"termsofuse.bypassNotification\", true);\n"
}

/// The `chrome/userChrome.css` of a web app's profile: the address bar, the
/// tabs and the bookmarks bar are hidden, so the window shows the website.
pub fn user_chrome() -> &'static str {
    "#nav-bar, #TabsToolbar, #PersonalToolbar { visibility: collapse !important; }\n"
}

/// The value of `key` in the `[Desktop Entry]` lines of `text`, if any.
fn entry_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim())
    })
}

/// The web app a desktop file describes, if it carries the marker: its
/// name and its website.
fn read_app(id: &str, text: &str) -> Option<WebApp> {
    let url = entry_value(text, MARKER)?;
    let name = entry_value(text, "Name").unwrap_or(url);
    Some(WebApp {
        id: id.to_string(),
        name: name.to_string(),
        url: url.to_string(),
    })
}

/// The web apps of the person, the desktop files in `apps_dir` that carry
/// the marker, by name. A folder that cannot be read has none.
fn list_in(apps_dir: &Path) -> Vec<WebApp> {
    let Ok(entries) = fs::read_dir(apps_dir) else {
        return Vec::new();
    };
    let mut apps: Vec<WebApp> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "desktop") {
                return None;
            }
            let id = path.file_stem()?.to_string_lossy().to_string();
            let text = fs::read_to_string(&path).ok()?;
            read_app(&id, &text)
        })
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// Writes the profile of `id` under `profiles` and the desktop file in
/// `apps_dir`, through a `.new` file and a rename. Refuses an id that is
/// already there, with the words that say how to remove it.
fn add_in(profiles: &Path, apps_dir: &Path, url: &str, name: &str) -> Result<WebApp> {
    let id = id_for(name);
    let desktop = apps_dir.join(format!("{id}.desktop"));
    if desktop.exists() {
        bail!(
            "{}",
            trf(
                "a web app named {name} is already there ({id}); remove it first with edel web-app remove {name}",
                &[("name", name), ("id", &id)],
            )
        );
    }
    let profile = profiles.join(&id);
    let shown = profile.display().to_string();
    fs::create_dir_all(profile.join("chrome")).with_context(|| {
        trf(
            "could not make the web app's profile in {path}",
            &[("path", &shown)],
        )
    })?;
    fs::write(profile.join("user.js"), profile_prefs()).with_context(|| {
        trf(
            "could not write {path}",
            &[("path", &profile.join("user.js").display().to_string())],
        )
    })?;
    fs::write(profile.join("chrome").join("userChrome.css"), user_chrome()).with_context(|| {
        trf(
            "could not write {path}",
            &[(
                "path",
                &profile
                    .join("chrome")
                    .join("userChrome.css")
                    .display()
                    .to_string(),
            )],
        )
    })?;
    fs::create_dir_all(apps_dir).with_context(|| {
        trf(
            "could not make the applications folder {path}",
            &[("path", &apps_dir.display().to_string())],
        )
    })?;
    let text = desktop_text(name, url, &id, &profile);
    let new = apps_dir.join(format!("{id}.desktop.new"));
    fs::write(&new, text).with_context(|| {
        trf(
            "could not write {path}",
            &[("path", &new.display().to_string())],
        )
    })?;
    fs::rename(&new, &desktop).with_context(|| {
        trf(
            "could not put {path} in place",
            &[("path", &desktop.display().to_string())],
        )
    })?;
    Ok(WebApp {
        id,
        name: name.to_string(),
        url: url.to_string(),
    })
}

/// Removes the web app `key` (its name or its id) from `apps_dir` and its
/// profile from `profiles`. Only a desktop file that carries the marker is
/// removed; any other file with that id is left alone.
fn remove_in(profiles: &Path, apps_dir: &Path, key: &str) -> Result<WebApp> {
    let id = if key.starts_with(ID_PREFIX) {
        key.to_string()
    } else {
        id_for(key)
    };
    let desktop = apps_dir.join(format!("{id}.desktop"));
    let text = match fs::read_to_string(&desktop) {
        Ok(text) => text,
        Err(_) => bail!(
            "{}",
            trf(
                "no web app named {key}; edel web-app list shows yours",
                &[("key", key)],
            )
        ),
    };
    let Some(app) = read_app(&id, &text) else {
        bail!(
            "{}",
            trf(
                "{key} is not a web app made by edel; it is left alone",
                &[("key", key)],
            )
        );
    };
    fs::remove_file(&desktop).with_context(|| {
        trf(
            "could not remove {path}",
            &[("path", &desktop.display().to_string())],
        )
    })?;
    let profile = profiles.join(&id);
    if profile.exists() {
        fs::remove_dir_all(&profile).with_context(|| {
            trf(
                "could not remove the profile {path}",
                &[("path", &profile.display().to_string())],
            )
        })?;
    }
    Ok(app)
}

/// The folder web apps' profiles are kept in: a folder inside Firefox's
/// Flatpak data, which its sandbox can write (`places`). None without a
/// home folder.
fn profiles_dir() -> Result<PathBuf> {
    places::web_apps_profiles_dir().with_context(|| {
        tr("could not find the home folder; web apps are made for the person who runs edel web-app")
    })
}

/// The folder of the person's desktop files; None without a home folder.
fn apps_dir() -> Result<PathBuf> {
    places::person_applications_dir().with_context(|| {
        tr("could not find the home folder; web apps are made for the person who runs edel web-app")
    })
}

/// Makes `url` a web app of the person's, called `name` or the website's
/// host, with its own profile and desktop file; the launcher lists it at
/// once. Refuses a name that is taken, and text that is not a website.
pub fn add(url: &str, name: Option<&str>) -> Result<WebApp> {
    if !is_url(url.trim()) {
        bail!(
            "{}",
            trf(
                "{url} is not a web address; write it as example.org or https://example.org",
                &[("url", url)],
            )
        );
    }
    let url = normalise(url);
    let name = match name.map(str::trim) {
        Some(name) if name.is_empty() || name.chars().any(char::is_control) => bail!(
            "{}",
            trf(
                "\"{name}\" is not a name for a web app; use letters and numbers, such as Mail",
                &[("name", name)],
            )
        ),
        Some(name) => name.to_string(),
        None => match name_from_url(&url) {
            Some(host) => host,
            None => bail!(
                "{}",
                trf(
                    "could not make a name from {url}; give one with --name NAME",
                    &[("url", &url)],
                )
            ),
        },
    };
    add_in(&profiles_dir()?, &apps_dir()?, &url, &name)
}

/// Removes the person's web app `key`, its name or its id: the desktop
/// file and the profile folder. Returns the app removed.
pub fn remove(key: &str) -> Result<WebApp> {
    remove_in(&profiles_dir()?, &apps_dir()?, key.trim())
}

/// The person's web apps, by name; none when they have none.
pub fn list() -> Vec<WebApp> {
    apps_dir().map(|dir| list_in(&dir)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of this test's own, under the temp directory, with the
    /// process id so parallel runs do not meet.
    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-web-apps-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_website_is_named_by_its_host_without_www() {
        assert_eq!(
            name_from_url("https://www.example.org/x").as_deref(),
            Some("example.org")
        );
        assert_eq!(
            name_from_url("http://Mail.Example.ORG:8080/a?b=c").as_deref(),
            Some("mail.example.org")
        );
        assert_eq!(
            name_from_url("https://user@example.org").as_deref(),
            Some("example.org")
        );
        assert_eq!(name_from_url("https://www.").as_deref(), None);
        assert_eq!(name_from_url("ftp://example.org"), None);
        assert_eq!(name_from_url("https://"), None);
        assert_eq!(name_from_url("https:///path"), None);
        assert_eq!(name_from_url("https://.org"), None);
        assert_eq!(name_from_url("example.org"), None);
    }

    #[test]
    fn text_is_a_website_when_it_is_one_word_with_a_host_and_a_tld() {
        for url in [
            "example.org",
            "https://example.org/x",
            "http://a.b",
            "www.example.org/path?q=1",
            "my-site.co.uk",
            "https://localhost/",
        ] {
            assert!(is_url(url), "{url} should be a website");
        }
        for text in [
            "",
            "example",
            "hello world",
            "example.org bar",
            "ftp://example.org",
            "https://",
            "1.2.3.4",
            "example.org.",
            "-bad.org",
            "bad..org",
            "under_score.org",
            "firefox",
        ] {
            assert!(!is_url(text), "{text:?} should not be a website");
        }
    }

    #[test]
    fn a_typed_address_gets_https_unless_it_has_a_scheme() {
        assert_eq!(normalise("example.org"), "https://example.org");
        assert_eq!(normalise("  example.org/a "), "https://example.org/a");
        assert_eq!(normalise("http://example.org"), "http://example.org");
        assert_eq!(normalise("https://example.org"), "https://example.org");
    }

    #[test]
    fn the_id_is_the_name_with_dashes() {
        assert_eq!(id_for("example.org"), "edel-web-example-org");
        assert_eq!(id_for("Mail App!!"), "edel-web-mail-app");
        assert_eq!(id_for("  a..b--c  "), "edel-web-a-b-c");
        assert_eq!(id_for("Café"), "edel-web-caf");
    }

    #[test]
    fn the_desktop_file_runs_firefox_with_its_profile_and_id() {
        let text = desktop_text(
            "example.org",
            "https://example.org/a",
            "edel-web-example-org",
            Path::new("/home/ci/.var/app/org.mozilla.firefox/edel-web-apps/edel-web-example-org"),
        );
        assert!(text.starts_with("[Desktop Entry]\nType=Application\nName=example.org\n"));
        assert!(text.contains(
            "Exec=flatpak run org.mozilla.firefox --new-instance --no-remote --name edel-web-example-org --profile /home/ci/.var/app/org.mozilla.firefox/edel-web-apps/edel-web-example-org https://example.org/a\n"
        ));
        assert!(text.contains("\nStartupWMClass=edel-web-example-org\n"));
        assert!(text.contains("\nX-Edel-Web-App=https://example.org/a\n"));
        assert!(text.contains("\nComment=https://example.org/a\n"));
        assert!(text.contains("\nIcon=web-browser\n"));
    }

    #[test]
    fn a_website_with_reserved_characters_is_quoted() {
        let text = desktop_text(
            "x",
            "https://example.org/?a=1&b=$c",
            "edel-web-x",
            Path::new("/p"),
        );
        assert!(text.contains("--profile /p \"https://example.org/?a=1&b=%%24c\"\n"));
    }

    #[test]
    fn a_profile_with_spaces_is_quoted_and_a_percent_is_doubled() {
        let text = desktop_text(
            "x",
            "https://example.org/a%20b",
            "edel-web-x",
            Path::new("/home/a b/profile"),
        );
        assert!(text.contains("--profile \"/home/a b/profile\" https://example.org/a%%20b\n"));
        assert!(text.contains("X-Edel-Web-App=https://example.org/a%20b\n"));
    }

    #[test]
    fn the_profile_hides_the_toolbars_and_skips_the_first_run_questions() {
        assert!(
            profile_prefs().contains(
                "user_pref(\"toolkit.legacyUserProfileCustomizations.stylesheets\", true);"
            )
        );
        assert!(
            profile_prefs().contains("user_pref(\"browser.shell.checkDefaultBrowser\", false);")
        );
        assert!(profile_prefs().contains("user_pref(\"browser.tabs.inTitlebar\", 0);"));
        for welcome in [
            "user_pref(\"browser.aboutwelcome.enabled\", false);",
            "user_pref(\"browser.preonboarding.enabled\", false);",
            "user_pref(\"termsofuse.bypassNotification\", true);",
        ] {
            assert!(profile_prefs().contains(welcome), "{welcome}");
        }
        assert!(user_chrome().contains("#nav-bar, #TabsToolbar, #PersonalToolbar"));
    }

    #[test]
    fn a_web_app_is_added_listed_and_removed_with_its_profile() {
        let root = scratch("add-list-remove");
        let profiles = root.join("profiles");
        let apps = root.join("applications");
        let added = add_in(&profiles, &apps, "https://example.org", "example.org").unwrap();
        assert_eq!(added.id, "edel-web-example-org");
        let profile = profiles.join(&added.id);
        assert!(profile.join("user.js").is_file());
        assert_eq!(
            fs::read_to_string(profile.join("chrome").join("userChrome.css")).unwrap(),
            user_chrome()
        );
        assert!(apps.join("edel-web-example-org.desktop").is_file());
        assert!(!apps.join("edel-web-example-org.desktop.new").exists());
        assert_eq!(list_in(&apps), vec![added.clone()]);
        // A second one with the same name is refused, naming it.
        let again = add_in(&profiles, &apps, "https://example.org", "example.org")
            .unwrap_err()
            .to_string();
        assert!(again.contains("example.org") && again.contains("edel web-app remove example.org"));
        let removed = remove_in(&profiles, &apps, "example.org").unwrap();
        assert_eq!(removed, added);
        assert!(!apps.join("edel-web-example-org.desktop").exists());
        assert!(!profile.exists());
        assert!(list_in(&apps).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_takes_an_id_too_and_leaves_other_files_alone() {
        let root = scratch("remove-only-ours");
        let profiles = root.join("profiles");
        let apps = root.join("applications");
        fs::create_dir_all(&apps).unwrap();
        // A desktop file of someone else's with an id a web app would take.
        fs::write(
            apps.join("edel-web-foo.desktop"),
            "[Desktop Entry]\nType=Application\nName=Foo\nExec=foo\n",
        )
        .unwrap();
        let error = remove_in(&profiles, &apps, "edel-web-foo")
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a web app made by edel"));
        assert!(apps.join("edel-web-foo.desktop").exists());
        assert!(remove_in(&profiles, &apps, "nothing").is_err());
        add_in(&profiles, &apps, "https://a.example", "a.example").unwrap();
        let by_id = remove_in(&profiles, &apps, "edel-web-a-example").unwrap();
        assert_eq!(by_id.url, "https://a.example");
        assert!(apps.join("edel-web-foo.desktop").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_that_is_not_one_is_refused_before_anything_is_written() {
        assert!(add("not a site", None).is_err());
        assert!(add("example.org", Some("bad\nname")).is_err());
        assert!(add("example.org", Some("  ")).is_err());
    }

    #[test]
    fn the_listing_reads_only_marked_files_in_name_order() {
        let root = scratch("listing");
        let apps = root.join("applications");
        fs::create_dir_all(&apps).unwrap();
        fs::write(
            apps.join("edel-web-b-org.desktop"),
            desktop_text("Beta", "https://b.org", "edel-web-b-org", Path::new("/p")),
        )
        .unwrap();
        fs::write(
            apps.join("edel-web-a-org.desktop"),
            desktop_text("alpha", "https://a.org", "edel-web-a-org", Path::new("/p")),
        )
        .unwrap();
        fs::write(apps.join("notes.txt"), "Name=x").unwrap();
        let names: Vec<String> = list_in(&apps).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["alpha", "Beta"]);
        assert!(list_in(&root.join("missing")).is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
