//! What an NVIDIA card can do on the open driver (roadmap M8.39c), in
//! plain words, for `edel report` and Settings' About. The card's
//! generation comes from its PCI device id, which anyone can read, so the
//! same words show without root: NVIDIA numbers each generation's chips in
//! its own range. Turing and newer (GTX 16, RTX 20 and later) run at full
//! clocks with nouveau and NVIDIA's GSP firmware, which the
//! `graphics-nvidia` pack carries (M8.39a); Maxwell and Pascal work but
//! stay at their slowest clocks, as NVIDIA lets only its own driver raise
//! them; older cards get the basics.

use crate::i18n::tr;

/// NVIDIA's PCI vendor id, as sysfs writes it.
pub const VENDOR: &str = "0x10de";

/// What a generation of NVIDIA cards can do on the open driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speed {
    /// Turing and newer: full clocks with GSP firmware.
    Full,
    /// Maxwell, Pascal and Volta: they work at their slowest clocks.
    Slow,
    /// Kepler and older: the basics.
    Basic,
}

/// The speed of the card with this PCI device id (`0x2504`).
pub fn speed(device: &str) -> Option<Speed> {
    let id = u16::from_str_radix(device.trim().trim_start_matches("0x"), 16).ok()?;
    Some(match id {
        0x1e00.. => Speed::Full,
        0x1340..=0x1dff => Speed::Slow,
        _ => Speed::Basic,
    })
}

/// What the card with this PCI class, device id and bound driver can do,
/// in one sentence, or none for a device that is not NVIDIA's graphics.
pub fn says(vendor: &str, class: &str, device: &str, driver: &str) -> Option<String> {
    // Display controllers are class 0x03xxxx.
    if !vendor.trim().eq_ignore_ascii_case(VENDOR) || !class.trim().starts_with("0x03") {
        return None;
    }
    let speed = speed(device)?;
    if driver.trim() != "nouveau" {
        return Some(tr(
            "this NVIDIA card has no driver yet; add NVIDIA graphics (graphics-nvidia) in Settings, Add-ons, or with edel settings set addons.installed+=graphics-nvidia, then restart",
        )
        .into());
    }
    let words = match speed {
        Speed::Full => tr(
            "this NVIDIA card (GTX 16, RTX 20 or newer) runs at full speed on the open driver, with NVIDIA's GSP firmware",
        ),
        Speed::Slow => tr(
            "this NVIDIA card (GTX 750 to GTX 10 series) works but stays at its slowest clocks, as NVIDIA lets only its own driver raise them: fine for the desktop and video, slow for games",
        ),
        Speed::Basic => tr(
            "this NVIDIA card (GTX 700 series or older) gets the basics on the open driver: the screen, video and light 3D",
        ),
    };
    Some(words.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_generation_reads_from_its_device_id() {
        // RTX 3060 (GA106), RTX 2060 (TU106), GTX 1650 (TU117), RTX 4090
        // (AD102), GTX 1080 (GP104), GTX 970 (GM204), GTX 750 Ti (GM107),
        // GTX 780 (GK110).
        assert_eq!(speed("0x2504"), Some(Speed::Full));
        assert_eq!(speed("0x1f08"), Some(Speed::Full));
        assert_eq!(speed("0x1f82"), Some(Speed::Full));
        assert_eq!(speed("0x2684"), Some(Speed::Full));
        assert_eq!(speed("0x1b80"), Some(Speed::Slow));
        assert_eq!(speed("0x13c2"), Some(Speed::Slow));
        assert_eq!(speed("0x1380"), Some(Speed::Slow));
        assert_eq!(speed("0x1004"), Some(Speed::Basic));
        assert_eq!(speed("not an id"), None);
    }

    #[test]
    fn says_what_the_card_can_do_or_how_to_give_it_a_driver() {
        let full = says("0x10de", "0x030000", "0x2504\n", "nouveau").unwrap();
        assert!(full.contains("full speed"), "{full}");
        let slow = says("0x10de", "0x030000", "0x1b80", "nouveau").unwrap();
        assert!(slow.contains("slowest clocks"), "{slow}");
        let basic = says("0x10de", "0x030200", "0x1004", "nouveau").unwrap();
        assert!(basic.contains("the basics"), "{basic}");
        let none = says("0x10de", "0x030000", "0x2504", "none").unwrap();
        assert!(none.contains("addons.installed+=graphics-nvidia"), "{none}");
        // NVIDIA's sound on the same card, and other makers, say nothing.
        assert_eq!(says("0x10de", "0x040300", "0x228e", "snd_hda_intel"), None);
        assert_eq!(says("0x8086", "0x030000", "0x9a49", "i915"), None);
    }
}
