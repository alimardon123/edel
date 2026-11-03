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
    list(places::CA_BUNDLE)
}

/// The authorities in the certificate list at `path`, if it holds any.
fn list(path: &str) -> Option<RootCerts> {
    let pem = fs::read(path).ok()?;
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

    /// Serves one HTTPS answer on a free local port with the test
    /// authority's certificate (`tests/tls`), and gives the address.
    fn serve_once() -> String {
        use rustls::pki_types::pem::PemObject;
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};
        use std::io::{Read, Write};
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tls");
        let certs = CertificateDer::pem_file_iter(format!("{dir}/server.pem"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = PrivateKeyDer::from_pem_file(format!("{dir}/server.key")).unwrap();
        let config = std::sync::Arc::new(
            rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // Each try is one connection: a refused one ends in the
            // handshake, a trusted one gets its answer.
            for stream in listener.incoming() {
                let Ok(mut tcp) = stream else { return };
                let Ok(mut tls) = rustls::ServerConnection::new(config.clone()) else {
                    return;
                };
                let mut tls = rustls::Stream::new(&mut tls, &mut tcp);
                let mut asked = [0u8; 1024];
                if tls.read(&mut asked).is_ok() {
                    let _ = tls.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    );
                    let _ = tls.flush();
                }
            }
        });
        format!("https://localhost:{port}/release.toml")
    }

    fn download(url: &str, roots: RootCerts) -> Result<String, ureq::Error> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(roots).build())
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build()
            .into();
        agent.get(url).call()?.body_mut().read_to_string()
    }

    #[test]
    fn a_mirror_whose_authority_is_only_in_the_machine_list_is_trusted() {
        let url = serve_once();
        let ours = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tls/ca.pem");
        // The machine's list holds the mirror's authority: one try, trusted.
        let got = either(list(ours), |roots| download(&url, roots));
        assert_eq!(got.unwrap(), "ok");
        // Without it, the built-in list does not know the mirror, and the
        // download is refused for its certificate, not for the network.
        let refused = either(None, |roots| download(&url, roots)).unwrap_err();
        assert!(untrusted(&refused), "{refused}");
    }
}
