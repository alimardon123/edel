//! `--wifi-join SSID PASSWORD` (the network case's Wi-Fi join): joins a
//! Wi-Fi network through `edel::network::join`, the same call Settings'
//! Join button makes, and prints what it answered, so the desktop test
//! proves the path Settings takes against the access points it makes with
//! mac80211_hwsim and hostapd, whatever way that call hands NetworkManager
//! the password. Run as ci, as Settings is.

use anyhow::Result;

pub fn join(ssid: &str, password: &str) -> Result<()> {
    match edel::network::join(ssid, false, Some(password)) {
        Ok(()) => println!("wifi join {ssid}: joined"),
        Err(e) => println!("wifi join {ssid}: refused: {e:#}"),
    }
    Ok(())
}
