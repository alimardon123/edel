//! The status area (M5.9a): one pill, `size.panel_control` high with the
//! controls' radius, holding how the machine is connected, how loud it is
//! and how full its battery is, each only where the machine has it (the
//! feature is installed and its daemon answers; `status.rs`). The icons
//! are the shell's own (`design/icons/net-*.svg`, `volume-*.svg`,
//! `battery*.svg`, `size.panel_glyph` across): the network by its kind
//! and, on Wi-Fi, its signal; the volume by its level or muted; the
//! battery filled to its charge with a bolt while it charges. While quick
//! settings are open the pill is filled with the panel text at 8.5 percent
//! as the menu button is for the launcher, and a click opens them, or
//! closes them.
//!
//! While an app uses the camera, the microphone or the screen (M6.14b) a
//! sign comes before the pill: an amber pill as high as the pill with each
//! kind's icon in it, and a click on it opens the in-use card
//! (`Action::InUse`), a click anywhere else `Action::Quick`.
//!
//! What it shows is text, tab-separated, `OPEN NET VOLUME BATTERY IN_USE`,
//! the parts split by U+001F: `1` while quick settings are open; the network
//! as `ICON WORDS`, the volume as `ICON PERCENT MUTED`, the battery as
//! `PERCENT CHARGING`, the kinds in use as their names, `camera`,
//! `microphone` and `screen`, joined by `,`; a part the machine lacks is
//! empty. A screen reader hears "Status: Wi-Fi Home 5G, volume 64 percent,
//! battery 82 percent", and "Camera and microphone in use, Status: ..." while
//! something uses them.

use accesskit::Role;
use edel::app_use::Kind;
use edel::i18n::{n_, tr, trf};
use edel::tokens::Tokens;

use super::{Action, Canvas, Input, Live, Widget};
use crate::in_use;
use crate::paint::{self, fill};
use crate::status::{Link, Status};

pub const WIDGET: Widget = Widget {
    name: "status",
    title: n_("Status"),
    needs: None,
    shows,
    width,
    draw,
    input: |canvas, shown, what| input(canvas.tokens, shown, what),
    parts: super::no_parts,
    role: Role::Button,
    label,
};

/// The room outside the pill, inside it at each end, and between icons,
/// in logical pixels.
const ROOM: f32 = 2.0;
const PAD: f32 = 10.0;
const GAP: f32 = 10.0;

/// The parts of what it shows.
#[derive(Debug, Default, PartialEq)]
struct Parts {
    open: bool,
    /// The icon and what a screen reader says of the connection.
    network: Option<(String, String)>,
    /// The icon, the percent and whether it is muted.
    volume: Option<(String, u32, bool)>,
    /// The percent and whether it charges.
    battery: Option<(u32, bool)>,
    /// The kinds an app uses now, in `Kind::ALL` order (M6.14b).
    in_use: Vec<Kind>,
}

const SEP: char = '\u{1f}';

/// The icon for a connection and the words for it.
fn network_part(status: &Status) -> Option<(&'static str, String)> {
    let network = status.network.as_ref()?;
    Some(match &network.link {
        Link::Wired => ("net-wired", tr("wired").to_string()),
        Link::Wifi { name, bars } => (
            match bars {
                0 | 1 => "net-wifi-1",
                2 => "net-wifi-2",
                _ => "net-wifi-3",
            },
            trf("Wi-Fi {name}", &[("name", name)]),
        ),
        Link::Connecting => ("net-wifi-1", tr("connecting").to_string()),
        Link::Offline if status.airplane() => ("airplane", tr("airplane mode").to_string()),
        Link::Offline => ("net-offline", tr("offline").to_string()),
    })
}

/// The icon of a volume: muted or silent, then three levels.
fn volume_icon(percent: u32, muted: bool) -> &'static str {
    match (muted, percent) {
        (true, _) | (_, 0) => "volume-muted",
        (_, 1..=33) => "volume-low",
        (_, 34..=66) => "volume-medium",
        _ => "volume-high",
    }
}

/// What the widget shows for `status`, `open` while quick settings are, and
/// the kinds an app uses now.
pub fn encode(status: &Status, open: bool, in_use: &[Kind]) -> String {
    let network = network_part(status)
        .map(|(icon, words)| format!("{icon}{SEP}{words}"))
        .unwrap_or_default();
    let volume = status
        .volume
        .as_ref()
        .map(|v| {
            format!(
                "{}{SEP}{}{SEP}{}",
                volume_icon(v.percent, v.muted),
                v.percent,
                u8::from(v.muted)
            )
        })
        .unwrap_or_default();
    let battery = status
        .battery
        .as_ref()
        .map(|b| format!("{}{SEP}{}", b.percent, u8::from(b.charging)))
        .unwrap_or_default();
    let in_use: Vec<&str> = in_use.iter().map(|kind| kind.name()).collect();
    format!(
        "{}\t{network}\t{volume}\t{battery}\t{}",
        u8::from(open),
        in_use.join(",")
    )
}

fn decode(shown: &str) -> Parts {
    let mut fields = shown.split('\t');
    let open = fields.next() == Some("1");
    let part = |text: Option<&str>| -> Vec<String> {
        match text {
            Some(t) if !t.is_empty() => t.split(SEP).map(str::to_string).collect(),
            _ => Vec::new(),
        }
    };
    let network = part(fields.next());
    let volume = part(fields.next());
    let battery = part(fields.next());
    let in_use = fields
        .next()
        .unwrap_or_default()
        .split(',')
        .filter_map(Kind::named)
        .collect();
    let number = |s: &String| s.parse::<u32>().unwrap_or(0);
    Parts {
        open,
        in_use,
        network: match &network[..] {
            [icon, words] => Some((icon.clone(), words.clone())),
            _ => None,
        },
        volume: match &volume[..] {
            [icon, percent, muted] => Some((icon.clone(), number(percent), muted == "1")),
            _ => None,
        },
        battery: match &battery[..] {
            [percent, charging] => Some((number(percent), charging == "1")),
            _ => None,
        },
    }
}

fn shows(live: &Live) -> String {
    encode(&live.status, live.quick, &live.in_use)
}

/// How many icons it holds.
fn count(parts: &Parts) -> usize {
    [
        parts.network.is_some(),
        parts.volume.is_some(),
        parts.battery.is_some(),
    ]
    .into_iter()
    .filter(|p| *p)
    .count()
}

/// What a screen reader hears: the sign's kinds in use, then the status.
fn label(shown: &str) -> String {
    let parts = decode(shown);
    let status = status_words(&parts);
    if parts.in_use.is_empty() {
        return status;
    }
    let sign = trf(
        "{what} in use",
        &[("what", &in_use::kinds_text(&parts.in_use))],
    );
    if status.is_empty() {
        sign
    } else {
        format!("{sign}, {status}")
    }
}

/// What a screen reader hears of the status, `Status: ...`, or nothing.
fn status_words(parts: &Parts) -> String {
    let mut said = Vec::new();
    if let Some((_, words)) = &parts.network {
        said.push(words.clone());
    }
    if let Some((_, percent, muted)) = &parts.volume {
        said.push(if *muted {
            tr("volume muted").to_string()
        } else {
            trf(
                "volume {percent} percent",
                &[("percent", &percent.to_string())],
            )
        });
    }
    if let Some((percent, charging)) = parts.battery {
        let percent = percent.to_string();
        said.push(if charging {
            trf(
                "battery {percent} percent, charging",
                &[("percent", &percent)],
            )
        } else {
            trf("battery {percent} percent", &[("percent", &percent)])
        });
    }
    if said.is_empty() {
        return String::new();
    }
    trf("Status: {parts}", &[("parts", &said.join(", "))])
}

/// The sign's pill, `kinds` icons wide: its padding, the icons and the gaps
/// between them, logical pixels.
fn pill_width(tokens: &Tokens, kinds: usize) -> f32 {
    let glyph = tokens.panel_glyph as f32;
    2.0 * PAD + kinds as f32 * glyph + (kinds as f32 - 1.0) * GAP
}

/// The sign's part of the widget: the room, the pill and the room, or none
/// while nothing is in use.
fn sign_width(tokens: &Tokens, kinds: usize) -> f32 {
    if kinds == 0 {
        0.0
    } else {
        ROOM + pill_width(tokens, kinds) + ROOM
    }
}

/// The status pill's part of the widget, none while it shows nothing.
fn status_width(tokens: &Tokens, parts: &Parts) -> f32 {
    let n = count(parts);
    if n == 0 {
        return 0.0;
    }
    let glyph = tokens.panel_glyph as f32;
    ROOM + PAD + n as f32 * glyph + (n - 1) as f32 * GAP + PAD + ROOM
}

/// Where the sign lies in the widget, from its left edge, and its width, in
/// logical pixels; none while nothing is in use. For the log CI reads
/// (M6.14b).
pub fn sign_place(tokens: &Tokens, shown: &str) -> Option<(f32, f32)> {
    let kinds = decode(shown).in_use.len();
    (kinds > 0).then(|| (ROOM, pill_width(tokens, kinds)))
}

/// A click on the sign opens the in-use card, centred over it; a click
/// anywhere else opens quick settings, or closes them.
fn input(tokens: &Tokens, shown: &str, input: Input) -> Option<Action> {
    let Input::Click(x, _) = input else {
        return None;
    };
    let kinds = decode(shown).in_use.len();
    if kinds > 0 && x < sign_width(tokens, kinds) {
        return Some(Action::InUse(ROOM + pill_width(tokens, kinds) / 2.0));
    }
    Some(Action::Quick)
}

/// Its width in logical pixels: none while it shows nothing.
pub fn logical_width(tokens: &Tokens, shown: &str) -> f32 {
    let parts = decode(shown);
    sign_width(tokens, parts.in_use.len()) + status_width(tokens, &parts)
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    logical_width(canvas.tokens, shown) * canvas.scale
}

/// Draws the sign's pill at `x`: the in-use colour, each kind's icon in its
/// text colour, with the status pill's padding and gap.
fn draw_sign(canvas: &mut Canvas, kinds: &[Kind], x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let (pw, ph) = (
        (pill_width(tokens, kinds.len()) * s).round(),
        (tokens.panel_control as f32 * s).round(),
    );
    let px = (x + ROOM * s).round();
    let py = canvas.top + ((canvas.height - ph) / 2.0).round();
    fill(
        canvas.pixmap,
        px,
        py,
        pw,
        ph,
        tokens.radius_control as f32 * s,
        tokens.in_use,
    );
    let glyph = (tokens.panel_glyph as f32 * s).round();
    let gy = py + ((ph - glyph) / 2.0).round();
    let mut gx = px + (PAD * s).round();
    let step = glyph + (GAP * s).round();
    for kind in kinds {
        paint::icon(
            canvas.pixmap,
            kind.icon(),
            glyph,
            gx,
            gy,
            tokens.in_use_text,
        );
        gx += step;
    }
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let parts = decode(shown);
    let tokens = canvas.tokens;
    let s = canvas.scale;
    if !parts.in_use.is_empty() {
        draw_sign(canvas, &parts.in_use, x);
    }
    let x = x + sign_width(tokens, parts.in_use.len()) * s;
    let n = count(&parts);
    if n == 0 {
        return;
    }
    let (pw, ph) = (
        ((status_width(tokens, &parts) - 2.0 * ROOM) * s).round(),
        (tokens.panel_control as f32 * s).round(),
    );
    let px = (x + ROOM * s).round();
    let py = canvas.top + ((canvas.height - ph) / 2.0).round();
    if parts.open {
        let lit = edel::tokens::Colour {
            a: 0.085,
            ..tokens.panel_text
        };
        fill(
            canvas.pixmap,
            px,
            py,
            pw,
            ph,
            tokens.radius_control as f32 * s,
            lit,
        );
    }
    let glyph = (tokens.panel_glyph as f32 * s).round();
    let gy = py + ((ph - glyph) / 2.0).round();
    let mut gx = px + (PAD * s).round();
    let step = glyph + (GAP * s).round();
    let ink = tokens.panel_text;
    if let Some((icon, _)) = &parts.network {
        paint::icon(canvas.pixmap, icon, glyph, gx, gy, ink);
        gx += step;
    }
    if let Some((icon, ..)) = &parts.volume {
        paint::icon(canvas.pixmap, icon, glyph, gx, gy, ink);
        gx += step;
    }
    if let Some((percent, charging)) = parts.battery {
        let halo = s.round().max(1.0) as i32;
        paint::battery(
            canvas.pixmap,
            glyph,
            (gx, gy),
            (percent, charging),
            ink,
            halo,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{Battery, Network, Volume};
    use edel::tokens::Tokens;

    fn full() -> Status {
        Status {
            network: Some(Network {
                link: Link::Wifi {
                    name: "Home 5G".into(),
                    bars: 3,
                },
                has_wifi: true,
                wifi_on: true,
            }),
            volume: Some(Volume {
                percent: 64,
                muted: false,
                output: "Speakers".into(),
            }),
            battery: Some(Battery {
                percent: 82,
                charging: false,
                line: String::new(),
                time: None,
            }),
            ..Status::default()
        }
    }

    #[test]
    fn what_it_shows_decodes_to_the_same_parts() {
        let parts = decode(&encode(&full(), true, &[]));
        assert!(parts.open);
        assert!(parts.in_use.is_empty());
        assert_eq!(
            parts.network,
            Some(("net-wifi-3".into(), "Wi-Fi Home 5G".into()))
        );
        assert_eq!(parts.volume, Some(("volume-medium".into(), 64, false)));
        assert_eq!(parts.battery, Some((82, false)));
        assert_eq!(decode(""), Parts::default());
    }

    #[test]
    fn a_screen_reader_hears_each_piece_there_is() {
        assert_eq!(
            label(&encode(&full(), false, &[])),
            "Status: Wi-Fi Home 5G, volume 64 percent, battery 82 percent"
        );
        let mut quiet = full();
        quiet.volume = Some(Volume {
            percent: 20,
            muted: true,
            output: String::new(),
        });
        quiet.battery.as_mut().unwrap().charging = true;
        assert_eq!(
            label(&encode(&quiet, false, &[])),
            "Status: Wi-Fi Home 5G, volume muted, battery 82 percent, charging"
        );
        // A machine with none of the three says nothing and takes no room.
        let nothing = encode(&Status::default(), false, &[]);
        assert_eq!(label(&nothing), "");
        assert_eq!(logical_width(&Tokens::built_in(), &nothing), 0.0);
    }

    #[test]
    fn it_is_as_wide_as_the_icons_it_holds() {
        let tokens = Tokens::built_in();
        let glyph = tokens.panel_glyph as f32;
        let wide = |status: &Status| logical_width(&tokens, &encode(status, false, &[]));
        let all = wide(&full());
        assert_eq!(all, 2.0 + 10.0 + 3.0 * glyph + 2.0 * 10.0 + 10.0 + 2.0);
        let mut two = full();
        two.battery = None;
        assert_eq!(all - wide(&two), glyph + GAP);
    }

    #[test]
    fn the_icons_follow_the_connection_the_volume_and_flight_mode() {
        let net = |link, has_wifi, wifi_on| Status {
            network: Some(Network {
                link,
                has_wifi,
                wifi_on,
            }),
            ..Status::default()
        };
        let icon = |s: &Status| network_part(s).map(|(i, _)| i);
        assert_eq!(icon(&net(Link::Wired, false, false)), Some("net-wired"));
        assert_eq!(icon(&net(Link::Offline, true, true)), Some("net-offline"));
        assert_eq!(icon(&net(Link::Offline, true, false)), Some("airplane"));
        let weak = Link::Wifi {
            name: "x".into(),
            bars: 1,
        };
        assert_eq!(icon(&net(weak, true, true)), Some("net-wifi-1"));
        assert_eq!(icon(&Status::default()), None);
        assert_eq!(volume_icon(0, false), "volume-muted");
        assert_eq!(volume_icon(80, true), "volume-muted");
        assert_eq!(volume_icon(20, false), "volume-low");
        assert_eq!(volume_icon(50, false), "volume-medium");
        assert_eq!(volume_icon(100, false), "volume-high");
    }

    #[test]
    fn a_click_opens_quick_settings_and_a_scroll_does_nothing() {
        let tokens = Tokens::built_in();
        let shown = encode(&full(), false, &[]);
        assert_eq!(
            input(&tokens, &shown, Input::Click(1.0, 98.0)),
            Some(Action::Quick)
        );
        assert_eq!(input(&tokens, &shown, Input::Scroll(1)), None);
    }

    #[test]
    fn the_sign_shows_each_kind_in_use_and_a_click_on_it_asks_for_the_card() {
        let tokens = Tokens::built_in();
        let shown = encode(&full(), false, &[Kind::Camera, Kind::Microphone]);
        let decoded = decode(&shown);
        assert_eq!(decoded.in_use, vec![Kind::Camera, Kind::Microphone]);
        assert_eq!(
            decoded.network,
            decode(&encode(&full(), false, &[])).network
        );
        let (at, width) = sign_place(&tokens, &shown).expect("a sign");
        assert_eq!(at, ROOM);
        assert_eq!(width, 2.0 * PAD + 2.0 * tokens.panel_glyph as f32 + GAP);
        // Each kind's icon is in the sign: its pill is wider by one icon and a gap.
        let one = encode(&full(), false, &[Kind::Screen]);
        assert_eq!(
            logical_width(&tokens, &shown) - logical_width(&tokens, &one),
            tokens.panel_glyph as f32 + GAP
        );
        let on_sign = Input::Click(at + width / 2.0, 100.0);
        assert_eq!(
            input(&tokens, &shown, on_sign),
            Some(Action::InUse(at + width / 2.0)),
            "the card hangs on the sign's middle"
        );
        let past = Input::Click(sign_width(&tokens, 2) + 4.0, 100.0);
        assert_eq!(input(&tokens, &shown, past), Some(Action::Quick));
        // With no status, the sign alone is the widget.
        let alone = encode(&Status::default(), false, &[Kind::Screen]);
        assert_eq!(logical_width(&tokens, &alone), sign_width(&tokens, 1));
        assert_eq!(
            input(&tokens, &alone, Input::Click(3.0, 9.0)),
            Some(Action::InUse(ROOM + pill_width(&tokens, 1) / 2.0))
        );
    }

    #[test]
    fn a_screen_reader_hears_the_sign_before_the_status() {
        let camera_and_mic = encode(&full(), false, &[Kind::Camera, Kind::Microphone]);
        assert_eq!(
            label(&camera_and_mic),
            "Camera and microphone in use, Status: Wi-Fi Home 5G, volume 64 percent, battery 82 percent"
        );
        let three = [Kind::Camera, Kind::Microphone, Kind::Screen];
        let all = encode(&full(), false, &three);
        assert!(label(&all).starts_with("Camera, microphone and screen in use, Status: "));
        let alone = encode(&Status::default(), false, &[Kind::Screen]);
        assert_eq!(label(&alone), "Screen in use");
    }

    #[test]
    fn the_sign_is_drawn_in_the_in_use_colour_before_the_pill() {
        let tokens = Tokens::built_in();
        let h = tokens.panel_height;
        let pixmap_of = |shown: &str| {
            let mut pixmap = tiny_skia::Pixmap::new(160, h).unwrap();
            pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 0));
            let mut canvas = Canvas {
                pixmap: &mut pixmap,
                tokens: &tokens,
                text: None,
                icons: None,
                scale: 1.0,
                top: 0.0,
                height: h as f32,
                dock: false,
                along_top: false,
            };
            draw(&mut canvas, shown, 0.0);
            pixmap
        };
        let shown = encode(&full(), false, &[Kind::Camera]);
        let pixmap = pixmap_of(&shown);
        let c = pixmap.pixel(2 + 4, h / 2).unwrap().demultiply();
        let amber = tokens.in_use;
        assert_eq!(
            [c.red(), c.green(), c.blue()],
            [amber.bytes()[0], amber.bytes()[1], amber.bytes()[2]],
            "the sign's pill, at its left"
        );
        // Without the sign, the same spot is the empty room.
        assert_eq!(
            pixmap_of(&encode(&full(), false, &[]))
                .pixel(2 + 4, h / 2)
                .unwrap()
                .alpha(),
            0
        );
    }

    #[test]
    fn an_open_pill_is_filled_and_a_closed_one_is_not() {
        let tokens = Tokens::built_in();
        let h = tokens.panel_height;
        let pill = |open: bool| {
            let mut pixmap = tiny_skia::Pixmap::new(120, h).unwrap();
            pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 0));
            let shown = encode(&full(), open, &[]);
            let mut canvas = Canvas {
                pixmap: &mut pixmap,
                tokens: &tokens,
                text: None,
                icons: None,
                scale: 1.0,
                top: 0.0,
                height: h as f32,
                dock: false,
                along_top: false,
            };
            draw(&mut canvas, &shown, 0.0);
            pixmap
        };
        // Four pixels in from the pill's left end, at its middle height.
        let at = |p: &tiny_skia::Pixmap| p.pixel(2 + 4, h / 2).unwrap().alpha();
        assert_eq!(at(&pill(false)), 0);
        assert!(at(&pill(true)) > 0);
    }

    #[test]
    fn a_charging_battery_is_filled_to_its_charge_and_the_bolt_is_cut_out() {
        let tokens = Tokens::built_in();
        let ink = tokens.panel_text;
        let draw = |percent: u32, charging: bool| {
            let mut pixmap = tiny_skia::Pixmap::new(18, 18).unwrap();
            paint::battery(&mut pixmap, 18.0, (0.0, 0.0), (percent, charging), ink, 1);
            pixmap
        };
        let covered = |p: &tiny_skia::Pixmap| p.pixels().iter().filter(|c| c.alpha() > 0).count();
        let (empty, half, full) = (draw(0, false), draw(50, false), draw(100, false));
        assert!(covered(&empty) < covered(&half) && covered(&half) < covered(&full));
        // The bolt over a full battery takes ink out as well as adding it.
        assert_ne!(draw(100, true).data(), full.data());
    }
}
