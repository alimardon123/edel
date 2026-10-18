//! `edel boot speakers` (roadmap M6.15), run at boot by the sound feature's
//! `edel-speakers`: links the speaker profile that matches this machine's
//! model (`edel::speakers`) into PipeWire's drop-in folder as
//! `places::SPEAKER_PROFILE`, or takes an old link out when none matches,
//! before any session starts PipeWire. The link points into the slot, so an
//! update's tuning applies with it and a rollback goes back with it. A file
//! there that is not our link is an administrator's own and is left alone.
//! It says on the console what it chose, one line, every boot.

use std::path::Path;

use anyhow::Result;
use edel::places::{SPEAKER_PROFILE, SPEAKERS_DIR};
use edel::sound::{self, LinkStep};
use edel::speakers;

pub fn speakers() -> Result<()> {
    let dir = Path::new(SPEAKERS_DIR);
    let (models, notes) = speakers::tables_in(dir);
    for note in notes {
        eprintln!("warning: edel boot speakers: {note}");
    }
    let model = speakers::machine_model(Path::new("/sys"));
    let wanted = speakers::wanted(&models, model.as_ref(), dir);
    let link = Path::new(SPEAKER_PROFILE);
    let now = sound::link_now(link);
    let named = model
        .as_ref()
        .map(|(vendor, product)| format!("{vendor} {product}"))
        .unwrap_or_else(|| "this machine, whose firmware names no model,".into());
    let step = sound::link_step(now.as_ref().map(|n| n.as_deref()), wanted.as_deref());
    if step == LinkStep::Keep && now == Some(None) {
        println!(
            "edel boot speakers: {SPEAKER_PROFILE} is not a link of ours, so it is left as it is"
        );
    }
    sound::take_link_step(link, &step)?;
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
