//! `--activate FILE` (the whole-system review of 2026-10-25): once FILE
//! exists, the window asks to come forward with an xdg-activation token it
//! made itself, with no click or key behind it, as an app on a timer
//! would, and prints `activation asked`. With `--sandboxed` it does so from
//! inside a security context, as a Flatpak app, so a test can see that
//! only the person's own programs are let through.

use anyhow::{Context, Result};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::xdg::activation::v1::client::{
    xdg_activation_token_v1::{self, XdgActivationTokenV1},
    xdg_activation_v1::XdgActivationV1,
};

use crate::Client;

/// The compositor's xdg-activation global.
pub fn bind(globals: &GlobalList, qh: &QueueHandle<Client>) -> Result<XdgActivationV1> {
    globals
        .bind(qh, 1..=1, ())
        .context("the compositor offers no xdg_activation_v1")
}

/// Asks for a token for `surface`, with no serial, and activates `surface`
/// with it once it is given.
pub fn ask(activation: &XdgActivationV1, qh: &QueueHandle<Client>, surface: &WlSurface) {
    let token = activation.get_activation_token(qh, (activation.clone(), surface.clone()));
    token.set_surface(surface);
    token.commit();
    println!("activation asked");
}

impl Dispatch<XdgActivationV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationTokenV1, (XdgActivationV1, WlSurface)> for Client {
    fn event(
        _: &mut Self,
        token: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        (activation, surface): &(XdgActivationV1, WlSurface),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_activation_token_v1::Event::Done { token: given } = event {
            activation.activate(given, surface);
            token.destroy();
        }
    }
}
