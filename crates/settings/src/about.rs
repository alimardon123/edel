//! The About page (M5.6a): this release, read from `/usr/lib/os-release`
//! as `edel` writes it at image build.

use adw::prelude::*;

const OS_RELEASE: &str = "/usr/lib/os-release";

pub fn page() -> adw::PreferencesPage {
    let release = std::fs::read_to_string(OS_RELEASE).unwrap_or_default();
    let page = adw::PreferencesPage::builder()
        .title("About")
        .icon_name("help-about-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("This system")
        .build();
    for (title, key) in [
        ("System", "PRETTY_NAME"),
        ("Version", "VERSION_ID"),
        ("Channel", "EDEL_CHANNEL"),
    ] {
        let value = field(&release, key).unwrap_or_else(|| "Unknown".into());
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(value)
            .subtitle_selectable(true)
            .build();
        row.add_css_class("property");
        group.add(&row);
    }
    page.add(&group);
    page
}

/// `key`'s value in os-release text, its quotes taken off.
fn field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_come_out_of_os_release_without_quotes() {
        let text = "NAME=\"Edel OS\"\nPRETTY_NAME=\"Edel OS 2026.10\"\nVERSION_ID=2026.10.1\nEDEL_CHANNEL=preview\n";
        assert_eq!(
            field(text, "PRETTY_NAME").as_deref(),
            Some("Edel OS 2026.10")
        );
        assert_eq!(field(text, "VERSION_ID").as_deref(), Some("2026.10.1"));
        assert_eq!(field(text, "NAME").as_deref(), Some("Edel OS"));
        assert_eq!(field(text, "BUILD_ID"), None);
    }
}
