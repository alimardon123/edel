//! Which certificate authorities `edel` trusts for an `https://` download
//! (roadmap M6.6b, M8.40): the machine's own list, `places::CA_BUNDLE`,
//! where a company adds the authority of its own mirror or settings
//! server, and the list built into the tool (Mozilla's, through
//! webpki-roots) when the machine has none.
//!
//! The updater also falls back to the built-in list when the machine's
//! list does not know a server ([`get_either`]): a list a person edited
//! under `/etc` is kept on `/data` across updates, so it can fall behind
//! the authorities GitHub moves to. `release.toml`'s signature stays the
//! real check of an update; the authorities only let the download start.

use std::fs;
use std::io;

use ureq::tls::{PemItem, RootCerts, parse_pem};

use edel::places;

/// The machine's authorities, or none when `places::CA_BUNDLE` is missing,
/// unreadable or holds no certificate.
fn machine() -> Option<RootCerts> {
    let pem = fs::read(places::CA_BUNDLE).ok()?;
    let certs: Vec<_> = parse_pem(&pem)
        .filter_map(|item| match item {
            Ok(PemItem::Certificate(cert)) => Some(cert.to_owned()),
            _ => None,
        })
        .collect();
    (!certs.is_empty()).then(|| RootCerts::new_with_certs(&certs))
}

/// The authorities a download is checked against: the machine's, else the
/// built-in ones.
pub fn roots() -> RootCerts {
    machine().unwrap_or(RootCerts::WebPki)
}

/// Whether a request failed because the server's certificate did not check
/// out against the authorities given (rustls reports it as invalid data),
/// rather than because the network or the server did.
fn untrusted(err: &ureq::Error) -> bool {
    match err {
        ureq::Error::Io(io) => io.kind() == io::ErrorKind::InvalidData,
        ureq::Error::Rustls(_) | ureq::Error::Tls(_) => true,
        _ => false,
    }
}

/// Runs `request` with the machine's authorities and, when the server's
/// certificate is one they do not know, once more with the built-in ones
/// (M8.40). A machine with no list of its own asks once, with the built-in
/// ones. Only a certificate's failure tries again, so an offline machine
/// still fails after one try.
pub fn get_either<T>(
    request: impl Fn(RootCerts) -> Result<T, ureq::Error>,
) -> Result<T, ureq::Error> {
    either(machine(), request)
}

/// [`get_either`] with the machine's authorities `own`.
fn either<T>(
    own: Option<RootCerts>,
    request: impl Fn(RootCerts) -> Result<T, ureq::Error>,
) -> Result<T, ureq::Error> {
    match own {
        None => request(RootCerts::WebPki),
        Some(own) => match request(own) {
            Err(err) if untrusted(&err) => request(RootCerts::WebPki),
            other => other,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_certificate_that_does_not_check_out_tries_the_built_in_list() {
        let bad_certificate = ureq::Error::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid peer certificate: UnknownIssuer",
        ));
        assert!(untrusted(&bad_certificate));
        assert!(untrusted(&ureq::Error::Tls("no certificate")));
        assert!(!untrusted(&ureq::Error::HostNotFound));
        assert!(!untrusted(&ureq::Error::ConnectionFailed));
        assert!(!untrusted(&ureq::Error::StatusCode(404)));
        assert!(!untrusted(&ureq::Error::Io(io::Error::from(
            io::ErrorKind::ConnectionRefused
        ))));
    }

    #[test]
    fn a_refused_certificate_is_asked_again_with_the_built_in_list_and_nothing_else_is() {
        use std::cell::RefCell;
        // The answers a fake server gives, one per try, and the lists tried.
        let run = |answers: Vec<Result<u8, ureq::Error>>, own: Option<RootCerts>| {
            let answers = RefCell::new(answers.into_iter());
            let tried = RefCell::new(Vec::new());
            let request = |roots: RootCerts| {
                tried.borrow_mut().push(matches!(roots, RootCerts::WebPki));
                answers.borrow_mut().next().unwrap()
            };
            let result = either(own, request);
            (result.ok(), tried.into_inner())
        };
        let own = || Some(RootCerts::new_with_certs(&[]));
        let refused = || ureq::Error::Io(io::Error::new(io::ErrorKind::InvalidData, "x"));
        // The machine's list is tried first; a refused certificate tries
        // the built-in list.
        assert_eq!(run(vec![Ok(1)], own()), (Some(1), vec![false]));
        assert_eq!(
            run(vec![Err(refused()), Ok(2)], own()),
            (Some(2), vec![false, true])
        );
        // Offline is offline: no second try.
        assert_eq!(
            run(vec![Err(ureq::Error::HostNotFound)], own()),
            (None, vec![false])
        );
        // No list of its own: the built-in one, once.
        assert_eq!(run(vec![Err(refused())], None), (None, vec![true]));
    }
}
