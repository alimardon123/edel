# The decided boards, as source

The boards behind the pictures in `docs/mockups/shell/`, kept so any
session can change them: each `NAME.dc.html` is one board, plain HTML
and CSS between `<helmet>` (the head) and `</x-dc>`, and `canvas.json`
places them on one page. Alimardon's design canvas, a private claude.ai
artifact, holds the same boards; a session on another account cannot
open it, so it works from these files.

To see a board, take its head and body into a page and open it in a
browser, or render it to a picture with headless Chromium:

```sh
b=DecLaptop; w=1280; h=800    # its size is in canvas.json
python3 - "$b" <<'PY'
import re, sys
s = open(sys.argv[1] + '.dc.html').read()
head = re.search(r'<helmet>(.*?)</helmet>', s, re.S).group(1)
body = re.search(r'</helmet>(.*?)</x-dc>', s, re.S).group(1)
open('/tmp/board.html', 'w').write('<!doctype html><meta charset="utf-8">' + head + '<body style="margin:0">' + body)
PY
chromium --headless=new --no-sandbox --hide-scrollbars --window-size=$w,$h --screenshot=/tmp/board.png file:///tmp/board.html
```

When a board changes, write its picture into `docs/mockups/shell/` in
the same pull request, as a JPEG at quality 85, and say in the README
there what was decided.
