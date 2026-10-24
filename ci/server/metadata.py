"""A stand-in for AWS's metadata address, IMDSv2 only (roadmap M7.4).

QEMU's guestfwd runs it for each connection the guest opens to
169.254.169.254:80, the request on standard input and the answer on
standard output: PUT /latest/api/token with a ttl header gives the token,
and GET /latest/user-data with it gives the file named first. Each request
is logged to the file named second.
"""

import sys

user_data_path, log_path = sys.argv[1], sys.argv[2]
TOKEN = "edel-ci-token"
stdin, stdout = sys.stdin.buffer, sys.stdout.buffer

request = stdin.readline().decode("latin-1").split()
headers = {}
while True:
    line = stdin.readline().decode("latin-1").strip()
    if not line:
        break
    name, _, value = line.partition(":")
    headers[name.strip().lower()] = value.strip()
if int(headers.get("content-length", "0") or 0):
    stdin.read(int(headers["content-length"]))

method, path = (request + ["", ""])[:2]
code, body = 404, b""
if method == "PUT" and path == "/latest/api/token" and "x-aws-ec2-metadata-token-ttl-seconds" in headers:
    code, body = 200, TOKEN.encode()
elif method == "PUT":
    code = 400
elif headers.get("x-aws-ec2-metadata-token") != TOKEN:
    code = 401
elif path == "/latest/user-data":
    code, body = 200, open(user_data_path, "rb").read()
with open(log_path, "a") as log:
    log.write(f"{method} {path} {code}\n")
reason = {200: "OK", 400: "Bad Request", 401: "Unauthorized", 404: "Not Found"}[code]
stdout.write(f"HTTP/1.1 {code} {reason}\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode() + body)
stdout.flush()
