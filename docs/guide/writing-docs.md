# Writing these docs

These pages are plain Markdown files in the repository's `docs/` folder. [mdBook](https://rust-lang.github.io/mdBook/), a small Rust program, turns them into this site, with search, a light and a dark look, and an edit button on every page.

## Change a page

1. Open the page on GitHub and press the edit button at the top right of the site's page, or edit the file in `docs/` in your checkout.
2. Write in the style the rest of the docs use: short plain sentences, British spelling for words such as colour, no dashes between words (commas, colons and full stops instead), and dates as 2026-10-06. The project's `CLAUDE.md` files hold the full rules.
3. Open a pull request. CI builds the site on every pull request and fails if a page, or a link to one, is missing.

## Add a page

1. Write `docs/NAME.md`, or `docs/guide/NAME.md` for a guide page.
2. Add one line for it to `docs/SUMMARY.md`, the site's table of contents, under the part it belongs to. A page not listed there is not on the site.
3. Add a row for it to `docs/README.md` if it is a design document.

## See it while you write

```sh
sh ci/site.sh serve
```

This installs the mdBook version CI uses, if you do not have it, and shows the site at <http://localhost:3000>, reloading as you save. `sh ci/site.sh build` writes it to `out/site/` and checks every link between pages.

## Pages written for you

Some pages are written from the one place that owns their facts, so they can never disagree with it (ADR-010's one owner decision). Never edit them by hand: change the owner, then run the command, and a cargo test fails until you do.

| Page | Written from | Command |
|---|---|---|
| [Commands](commands.md) | `edel`'s own command table, `crates/edel/src/main.rs` | `EDEL_WRITE_DOCS=1 cargo test -p edel command_reference` |
| [Keyboard shortcuts](../SHORTCUTS.md) | The shortcut table, `crates/edel/src/shortcuts.rs` | `EDEL_WRITE_DOCS=1 cargo test -p edel shortcuts` |
| `docs/theme/tokens.css` | The design tokens, `design/tokens.toml` | `EDEL_WRITE_DOCS=1 cargo test -p edel site_css` |

## The look

The site's colours, corners and font are the desktop's: `docs/theme/tokens.css` is written from the design tokens, and `docs/theme/edel.css` says where each one goes and holds no colour of its own. Change a token, and the desktop, its apps and this site all follow. mdBook's theme menu offers its five themes; the light ones take our light scheme and the dark ones our dark scheme.

## Where it is published

The site is built into `out/site/` on every pull request. Once Alimardon turns publishing on, every merge to `main` publishes it to <https://alimardon123.github.io/edel/> with GitHub Pages, which costs nothing ([Releases](../RELEASE.md)).
