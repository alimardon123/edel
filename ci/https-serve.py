#!/usr/bin/env python3
"""https-serve.py DIR CERT KEY (roadmap M6.6b, test only): serves DIR's
files over https on port 8443 with CERT and KEY, and over plain http on
port 8080, until it is killed. The install test's guest fetches the
throwaway authority over http to trust it, installs from the settings
file over https, and must refuse the same file over http.

Standard library only.
"""
import functools
import http.server
import ssl
import sys
import threading


def main():
    directory, cert, key = sys.argv[1:4]
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=directory)
    secure = http.server.ThreadingHTTPServer(("0.0.0.0", 8443), handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    secure.socket = context.wrap_socket(secure.socket, server_side=True)
    plain = http.server.ThreadingHTTPServer(("0.0.0.0", 8080), handler)
    threading.Thread(target=plain.serve_forever, daemon=True).start()
    secure.serve_forever()


if __name__ == "__main__":
    main()
