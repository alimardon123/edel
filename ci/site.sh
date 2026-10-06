#!/bin/sh
# The docs site (roadmap M8.10a): builds docs/ with mdBook into out/site/
# and checks every link between its pages.
#
#   sh ci/site.sh build     build and check, as CI does on every pull request
#   sh ci/site.sh serve     show it at http://localhost:3000 while you write
#   sh ci/site.sh publish   build, then put it on the gh-pages branch, which
#                           GitHub Pages serves, keeping channels/ (ci/channel.sh)
#
# Each installs the one mdBook version below with cargo when the mdbook on
# PATH is another. publish runs only from CI on main, once Alimardon sets
# the repository variable EDEL_SITE to yes (docs/RELEASE.md).
set -eu

# The mdBook version the site is built with: written here only.
mdbook_version=0.5.4

mode=${1:-}
case "$mode" in
build | serve | publish) ;;
*)
	echo "usage: sh ci/site.sh build|serve|publish" >&2
	exit 2
	;;
esac

if [ "$(mdbook --version 2>/dev/null || true)" != "mdbook v$mdbook_version" ]; then
	echo "installing mdBook $mdbook_version with cargo"
	cargo install --quiet --locked mdbook --version "$mdbook_version"
fi

if [ "$mode" = serve ]; then
	exec mdbook serve docs --open
fi

rm -rf out/site
mdbook build docs

# Every link between the site's pages, and every #anchor in one, must land:
# mdBook warns about neither.
python3 - out/site <<'EOF'
import html.parser, os, sys, urllib.parse

root = sys.argv[1]
skip = {"print.html", "toc.html", "404.html"}

class Page(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids, self.links = set(), []
    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if "id" in a:
            self.ids.add(a["id"])
        if tag == "a" and a.get("href"):
            self.links.append(a["href"])

pages = {}
for dirpath, _, files in os.walk(root):
    for name in files:
        if name.endswith(".html"):
            path = os.path.join(dirpath, name)
            page = Page()
            with open(path, encoding="utf-8") as f:
                page.feed(f.read())
            pages[os.path.normpath(path)] = page

broken = []
for path, page in pages.items():
    if os.path.basename(path) in skip:
        continue
    # The sidebar's links are mdBook's own, from SUMMARY.md, and checked
    # when it builds; the content's are ours.
    for href in page.links:
        url = urllib.parse.urlsplit(href)
        if url.scheme or href.startswith("//"):
            continue
        target = path if not url.path else os.path.normpath(os.path.join(os.path.dirname(path), urllib.parse.unquote(url.path)))
        if not os.path.exists(target):
            broken.append(f"{os.path.relpath(path, root)}: {href} (no such page)")
        elif url.fragment and target in pages and urllib.parse.unquote(url.fragment) not in pages[target].ids:
            broken.append(f"{os.path.relpath(path, root)}: {href} (no such heading)")

for line in sorted(set(broken)):
    print(f"FAIL: {line}")
if broken:
    sys.exit(1)
print(f"PASS: {len(pages)} pages, every link between them lands")
EOF

[ "$mode" = publish ] || exit 0

pages=$(mktemp -d)
rmdir "$pages"
cleanup() {
	git worktree remove --force "$pages" 2>/dev/null || true
	git branch -D -q "gh-pages-$$" 2>/dev/null || true
}
trap cleanup EXIT
if git fetch -q origin gh-pages 2>/dev/null; then
	git worktree add -q --detach "$pages" FETCH_HEAD
else
	git worktree add -q --orphan -b "gh-pages-$$" "$pages"
fi
# Everything but the update channels, which ci/channel.sh owns, is the site.
find "$pages" -mindepth 1 -maxdepth 1 ! -name .git ! -name channels -exec rm -rf {} +
cp -R out/site/. "$pages/"
# GitHub Pages serves the files as they are, without Jekyll.
touch "$pages/.nojekyll"
git -C "$pages" add -A
if git -C "$pages" diff --cached --quiet; then
	echo "the site is already up to date"
	exit 0
fi
git -C "$pages" -c user.name="github-actions[bot]" \
	-c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
	commit -qm "Publish the docs site from $(git rev-parse --short HEAD)"
git -C "$pages" push -q origin HEAD:refs/heads/gh-pages
echo "published the docs site to gh-pages"
