//! A cloud's user data as the machine's first settings file (roadmap M7.4,
//! ADR-009): `edel boot cloud`, run once the network is up by the `cloud`
//! feature's service, reads the instance's user data from its provider's
//! metadata address when the machine has no settings file yet, keeps it as
//! the settings file and restarts, so the boot applies it as it applies a
//! seed. The user data is the settings file itself: no cloud-init and no
//! cloud-config. It asks only where the firmware's DMI names a provider
//! below, so no other machine waits on the network for it (Reliable).
//!
//! The metadata address is plain `http://`, which a settings file is
//! otherwise never read from (ADR-006, `settings_source`): it is the
//! link-local address of the hypervisor the machine runs on, which no
//! other machine can answer, and every cloud serves it so.
//!
//! Each provider is one entry of [`PROVIDERS`]: the DMI vendor it shows and
//! how its user data is asked for.

use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Result, bail};

use edel::i18n::trf;
use edel::places;

use crate::machine;

/// The metadata address every provider here serves.
const METADATA: &str = "http://169.254.169.254";

/// How a provider gives its user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// A plain GET of this path.
    Get(&'static str),
    /// AWS's IMDSv2: a session token by PUT, then the GET with it.
    AwsToken,
}

/// A cloud provider: its name, the words its DMI `sys_vendor` starts with,
/// and how its user data is asked for.
struct Provider {
    name: &'static str,
    vendor: &'static str,
    ask: Ask,
}

/// The providers whose user data is read, by DMI vendor.
const PROVIDERS: [Provider; 3] = [
    Provider {
        name: "AWS",
        vendor: "Amazon EC2",
        ask: Ask::AwsToken,
    },
    Provider {
        name: "Hetzner",
        vendor: "Hetzner",
        ask: Ask::Get("/hetzner/v1/userdata"),
    },
    Provider {
        name: "OpenStack",
        vendor: "OpenStack",
        ask: Ask::Get("/openstack/latest/user_data"),
    },
];

/// The provider the firmware's DMI vendor names, if any.
fn provider(sys_vendor: &str) -> Option<&'static Provider> {
    let vendor = sys_vendor.trim();
    PROVIDERS.iter().find(|p| vendor.starts_with(p.vendor))
}

/// `edel boot cloud`: keeps a cloud's user data as the settings file and
/// restarts, or does nothing on a machine that has a settings file or is
/// not on a known cloud. It never fails the boot: what went wrong is said
/// in one line, and the machine starts as it is.
pub fn boot() -> Result<()> {
    let target = places::machine_settings();
    if places::found(&target).exists() {
        return Ok(());
    }
    let vendor = fs::read_to_string("/sys/class/dmi/id/sys_vendor").unwrap_or_default();
    let Some(provider) = provider(&vendor) else {
        return Ok(());
    };
    let text = match fetch(provider) {
        Ok(Some(text)) => text,
        Ok(None) => {
            say(&trf(
                "{provider} gave this machine no user data, so it starts without a settings file",
                &[("provider", provider.name)],
            ));
            return Ok(());
        }
        Err(err) => {
            say(&trf(
                "could not read {provider}'s user data: {why}; the machine starts without a settings file",
                &[("provider", provider.name), ("why", &format!("{err:#}"))],
            ));
            return Ok(());
        }
    };
    if let Err(err) = edel::settings::read(&text) {
        say(&trf(
            "{provider}'s user data is not a settings file this release can read: {why}",
            &[("provider", provider.name), ("why", &format!("{err:#}"))],
        ));
        return Ok(());
    }
    machine::write_whole(&target, text.as_bytes())?;
    say(&trf(
        "kept {provider}'s user data as {path}; restarting to start with it",
        &[
            ("provider", provider.name),
            ("path", &target.display().to_string()),
        ],
    ));
    wait_for_confirmed();
    crate::system_power::run(crate::system_power::Action::Restart)
}

/// One line on the console and in the system log.
fn say(line: &str) {
    println!("edel boot cloud: {line}");
    let _ = std::process::Command::new("logger")
        .args(["-t", "edel", &format!("edel boot cloud: {line}")])
        .status();
}

/// Waits up to a minute for the guard to confirm this boot's slot, so the
/// restart does not spend one of its tries.
fn wait_for_confirmed() {
    for _ in 0..600 {
        if Path::new(places::CONFIRMED).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The provider's user data, None when it has none (404). The network may
/// come up a little after this starts, so a failed connection is tried
/// again for up to a minute.
fn fetch(provider: &Provider) -> Result<Option<String>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut last = None;
    for _ in 0..12 {
        match ask(&agent, provider.ask) {
            Ok(found) => return Ok(found),
            Err(err) => last = Some(err),
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("no answer")))
}

/// One try at the user data.
fn ask(agent: &ureq::Agent, how: Ask) -> Result<Option<String>> {
    let mut get = match how {
        Ask::Get(path) => agent.get(format!("{METADATA}{path}")).call()?,
        Ask::AwsToken => {
            let mut answer = agent
                .put(format!("{METADATA}/latest/api/token"))
                .header("X-aws-ec2-metadata-token-ttl-seconds", "60")
                .send_empty()?;
            if answer.status().as_u16() != 200 {
                bail!("the token request was answered {}", answer.status());
            }
            let token = answer.body_mut().read_to_string()?;
            agent
                .get(format!("{METADATA}/latest/user-data"))
                .header("X-aws-ec2-metadata-token", token.trim())
                .call()?
        }
    };
    match get.status().as_u16() {
        200 => Ok(Some(
            get.body_mut()
                .with_config()
                .limit(crate::settings_source::MOST_BYTES)
                .read_to_string()?,
        )),
        404 => Ok(None),
        status => bail!("the metadata address answered {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_is_found_by_its_dmi_vendor() {
        assert_eq!(provider("Amazon EC2\n").map(|p| p.name), Some("AWS"));
        assert_eq!(
            provider("Hetzner").map(|p| p.ask),
            Some(Ask::Get("/hetzner/v1/userdata"))
        );
        assert_eq!(
            provider("OpenStack Foundation").map(|p| p.name),
            Some("OpenStack")
        );
        assert!(provider("QEMU").is_none());
        assert!(provider("LENOVO").is_none());
        assert!(provider("").is_none());
    }
}
