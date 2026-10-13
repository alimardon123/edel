//! `edel boot speakers` (roadmap M6.15), run at boot by the sound feature's
//! `edel-speakers`: links the speaker profile that matches this machine's
//! model (`edel::speakers`) into PipeWire's drop-in folder as
//! `places::SPEAKER_PROFILE`, or takes an old link out when none matches,
//! before any session starts PipeWire. The link points into the slot, so an
//! update's tuning applies with it and a rollback goes back with it. A file
//! there that is not our link is an administrator's own and is left alone.
//! It says on the console what it chose, one line, every boot.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use anyhow::{Context, Result};
use edel::places::{SPEAKER_PROFILE, SPEAKERS_DIR};
use edel::speakers;

/// What the link should become, given what is there now (`now`: a link's
/// target, `Some(None)` for a file that is not a link, `None` for nothing)
/// and the profile wanted.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Leave it: it is right, or it is not ours
    Keep,
    /// Make the link to this profile, replacing ours
    Link(std::path::PathBuf),
    /// Take our link out
    Remove,
}

fn step(now: Option<Option<&Path>>, wanted: Option<&Path>) -> Step {
    match (now, wanted) {
        // A plain file is an administrator's own tuning.
        (Some(None), _) => Step::Keep,
        (Some(Some(target)), Some(wanted)) if target == wanted => Step::Keep,
        (_, Some(wanted)) => Step::Link(wanted.to_path_buf()),
        (Some(Some(_)), None) => Step::Remove,
        (None, None) => Step::Keep,
    }
}

pub fn speakers() -> Result<()> {
    let dir = Path::new(SPEAKERS_DIR);
    let (models, notes) = speakers::tables_in(dir);
    for note in notes {
        eprintln!("warning: edel boot speakers: {note}");
    }
    let model = speakers::machine_model(Path::new("/sys"));
    let wanted = speakers::wanted(&models, model.as_ref(), dir);
    let link = Path::new(SPEAKER_PROFILE);
    let now = match fs::symlink_metadata(link) {
        Ok(meta) if meta.file_type().is_symlink() => Some(fs::read_link(link).ok()),
        Ok(_) => Some(None),
        Err(_) => None,
    };
    let named = model
        .as_ref()
        .map(|(vendor, product)| format!("{vendor} {product}"))
        .unwrap_or_else(|| "this machine, whose firmware names no model,".into());
    match step(now.as_ref().map(|n| n.as_deref()), wanted.as_deref()) {
        Step::Keep if now == Some(None) => println!(
            "edel boot speakers: {SPEAKER_PROFILE} is not a link of ours, so it is left as it is"
        ),
        Step::Keep => {}
        Step::Link(target) => {
            if let Some(parent) = link.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("could not make {}", parent.display()))?;
            }
            if now.is_some() {
                fs::remove_file(link)
                    .with_context(|| format!("could not replace {SPEAKER_PROFILE}"))?;
            }
            symlink(&target, link).with_context(|| format!("could not write {SPEAKER_PROFILE}"))?;
        }
        Step::Remove => {
            fs::remove_file(link).with_context(|| format!("could not remove {SPEAKER_PROFILE}"))?
        }
    }
    match wanted.as_deref().and_then(Path::file_stem) {
        Some(name) => println!(
            "edel boot speakers: {named} uses the speaker profile {}",
            name.to_string_lossy()
        ),
        None => println!(
            "edel boot speakers: no speaker profile for {named}, so its speakers play as they are"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_the_profile_once_takes_ours_out_and_leaves_a_file_alone() {
        let profile = Path::new("/usr/share/edel/speakers/ci-test.conf");
        let other = Path::new("/usr/share/edel/speakers/old.conf");
        assert_eq!(step(None, Some(profile)), Step::Link(profile.into()));
        assert_eq!(step(Some(Some(profile)), Some(profile)), Step::Keep);
        assert_eq!(
            step(Some(Some(other)), Some(profile)),
            Step::Link(profile.into())
        );
        assert_eq!(step(Some(Some(other)), None), Step::Remove);
        assert_eq!(step(None, None), Step::Keep);
        assert_eq!(
            step(Some(None), Some(profile)),
            Step::Keep,
            "an administrator's file"
        );
        assert_eq!(step(Some(None), None), Step::Keep);
    }
}
