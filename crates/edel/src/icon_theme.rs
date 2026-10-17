//! A freedesktop icon theme's `index.theme` (M8.21 part g), the one parser
//! of that file (ADR-010): the folders a theme has, their sizes and scales,
//! whether each holds app icons, and the themes it inherits. Std only, so
//! the compositor, shell-ui and Settings read it the same way the library
//! does. `app_icons` finds an app's icon through it, in the order the theme
//! spec gives.

use std::path::Path;

/// One folder of a theme, as its index names it.
pub struct Dir {
    /// The folder under the theme, such as `48x48/apps`.
    pub path: String,
    /// The folder's size in pixels: its `Size` times its `Scale`.
    pub size: u32,
    /// The folder's `Scale`, 1 unless the index says otherwise.
    pub scale: u32,
    /// Whether the folder's `Type` is `Scalable`.
    pub scalable: bool,
    /// Whether the folder holds app icons: its `Context` is `Applications`
    /// or `Apps`, or its last part is `apps`.
    pub apps: bool,
}

/// What a theme's index says.
pub struct Index {
    /// The themes it inherits, in the order written.
    pub inherits: Vec<String>,
    /// Its folders, in the order its `Directories` and `ScaledDirectories`
    /// list them; a folder with no parsable `Size` is left out.
    pub dirs: Vec<Dir>,
    /// `Hidden=true`: the theme is not one an app may use.
    pub hidden: bool,
}

/// Reads an index's text. Lines starting with `#` and blank lines are
/// skipped; keys and values are trimmed.
pub fn parse(text: &str) -> Index {
    let mut sections: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.trim(), Vec::new()));
        } else if let (Some((key, value)), Some((_, keys))) =
            (line.split_once('='), sections.last_mut())
        {
            keys.push((key.trim(), value.trim()));
        }
    }
    let get = |section: &str, key: &str| -> Option<&str> {
        let (_, keys) = sections.iter().find(|(name, _)| *name == section)?;
        keys.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    };
    let mut paths = comma_list(get("Icon Theme", "Directories"));
    for path in comma_list(get("Icon Theme", "ScaledDirectories")) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let dirs = paths
        .into_iter()
        .filter_map(|path| {
            let size: u32 = get(&path, "Size")?.parse().ok()?;
            let scale = get(&path, "Scale")
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|s| *s >= 1)
                .unwrap_or(1);
            let context = get(&path, "Context").unwrap_or("");
            let apps = context.eq_ignore_ascii_case("Applications")
                || context.eq_ignore_ascii_case("Apps")
                || path.rsplit('/').next() == Some("apps");
            Some(Dir {
                size: size.saturating_mul(scale),
                scale,
                scalable: get(&path, "Type") == Some("Scalable"),
                apps,
                path,
            })
        })
        .collect();
    Index {
        inherits: comma_list(get("Icon Theme", "Inherits")),
        dirs,
        hidden: get("Icon Theme", "Hidden").is_some_and(|v| v.eq_ignore_ascii_case("true")),
    }
}

/// The index of the theme in `theme_dir`, None when its `index.theme` cannot
/// be read.
pub fn read(theme_dir: &Path) -> Option<Index> {
    std::fs::read_to_string(theme_dir.join("index.theme"))
        .ok()
        .map(|text| parse(&text))
}

/// Whether an index makes an icon theme an app may use: not hidden, and
/// with at least one folder.
pub fn is_icon_theme(index: &Index) -> bool {
    !index.hidden && !index.dirs.is_empty()
}

/// A comma separated list, trimmed, with empty items and repeats dropped,
/// the order kept.
fn comma_list(value: Option<&str>) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for item in value.unwrap_or("").split(',').map(str::trim) {
        if !item.is_empty() && !items.iter().any(|i| i == item) {
            items.push(item.to_string());
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# A sample index (M8.21 part g).\n\
        [Icon Theme]\n\
        Name=Sample\n\
        Inherits= hicolor , , Adwaita,hicolor\n\
        Directories=16x16/apps,48x48/apps, 22x22/places,scalable/apps\n\
        ScaledDirectories=16x16@2/apps,nosize\n\
        Hidden=true\n\
        \n\
        [16x16/apps]\n\
        Size=16\n\
        Context=Applications\n\
        \n\
        [48x48/apps]\n\
        Size=48\n\
        Context=apps\n\
        \n\
        [22x22/places]\n\
        Size=22\n\
        Context=Places\n\
        \n\
        [scalable/apps]\n\
        Size=64\n\
        Type=Scalable\n\
        Context=Apps\n\
        \n\
        [16x16@2/apps]\n\
        Size=16\n\
        Scale=2\n\
        Context=Applications\n\
        \n\
        [nosize]\n\
        Context=Applications\n";

    #[test]
    fn reads_the_inherited_themes_and_each_folder() {
        let index = parse(SAMPLE);
        assert_eq!(index.inherits, ["hicolor", "Adwaita"]);
        assert!(index.hidden);
        let folders: Vec<(&str, u32, u32, bool, bool)> = index
            .dirs
            .iter()
            .map(|d| (d.path.as_str(), d.size, d.scale, d.scalable, d.apps))
            .collect();
        assert_eq!(
            folders,
            [
                ("16x16/apps", 16, 1, false, true),
                ("48x48/apps", 48, 1, false, true),
                ("22x22/places", 22, 1, false, false),
                ("scalable/apps", 64, 1, true, true),
                ("16x16@2/apps", 32, 2, false, true),
            ]
        );
    }

    #[test]
    fn a_scalable_folder_and_a_context_of_apps_are_read() {
        let index = parse(
            "[Icon Theme]\nDirectories=scalable/apps\n[scalable/apps]\nSize=48\nType=Scalable\nContext=Apps\n",
        );
        assert_eq!(index.dirs.len(), 1);
        assert!(index.dirs[0].scalable && index.dirs[0].apps);
        assert!(is_icon_theme(&index));
    }

    #[test]
    fn a_hidden_theme_or_one_with_no_folder_is_no_icon_theme() {
        assert!(!is_icon_theme(&parse(SAMPLE)));
        assert!(!is_icon_theme(&parse("[Icon Theme]\nName=Empty\n")));
        let plain = parse("[Icon Theme]\nDirectories=32x32/apps\n[32x32/apps]\nSize=32\n");
        assert!(is_icon_theme(&plain));
    }

    #[test]
    fn a_theme_directory_without_an_index_is_none() {
        let dir = std::env::temp_dir().join(format!("edel-icon-theme-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read(&dir).is_none());
        std::fs::write(
            dir.join("index.theme"),
            "[Icon Theme]\nDirectories=8x8/apps\n[8x8/apps]\nSize=8\n",
        )
        .unwrap();
        assert!(read(&dir).is_some_and(|i| i.dirs.len() == 1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
