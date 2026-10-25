# Security fixes

How Edel OS takes Alpine's security fixes, and how to read what every image is made of. For what CI publishes and the release key, see [RELEASE.md](RELEASE.md).

## Where fixes come from

Every image is built from the Alpine branch `features/base.toml` names (ADR-003), and a build takes the newest packages that branch has that day. So a fix reaches people as soon as an image is built after Alpine ships it: there is no lock file to bump, only a build to run. Two things make sure one runs:

- **Every change** builds every image again, and the build fails when an image lacks a fix Alpine already ships (`ci/bom.sh advisories`, M8.12).
- **The monthly rebuild** (M8.3) builds everything once a month even when nothing else changed, and turns it into a release.

## The monthly rebuild

The workflow "Monthly rebuild" (`.github/workflows/monthly.yml`) runs at 03:17 UTC on the 1st of each month, and from the Actions tab whenever someone starts it by hand (Run workflow), say after a serious advisory.

| Step | What happens |
|---|---|
| The rebuild starts | `ci/monthly.sh prepare` reads the newest digest of Alpine's container for the branch and writes it, with the date, into `features/base.toml`; it compares the newest published release's package lists (or the preview's) with the versions on the branch today |
| A pull request opens | The branch `monthly/YYYY-MM` and a pull request into `main`, whose body lists every package that changed, image by image, as `-` old and `+` new lines |
| CI runs | The workflow starts the full "CI" run on that branch: every image is built from the branch as it is that day and every test runs, as for any change. The run's summary also shows each image's packages against the preview (`ci/package-report.sh`) and its open advisories |
| It merges | Once all four checks are green, the pull request is merged like any other. Merging tags the merge `vYYYY.MM.0` (the month's first release; a second fix release that month is `.1`) and starts "CI" on the tag |
| A draft release | The tag's run builds, tests, signs and, when publishing is on (`EDEL_PUBLISH`, [RELEASE.md](RELEASE.md)), makes the draft release with its checklist; it is published as every release is |

The rebuild changes no look and no default: only the build container's digest and the packages move. Its lists are signed afresh, so a machine's release list never runs out (lists expire 60 days after signing).

A pull request or tag made by a workflow's own token starts no workflow, so the monthly workflow starts "CI" itself (`workflow_dispatch`), on the branch and on the tag. A dispatched run on a tag is signed and published as a pushed tag is; a dispatched run on a branch is a dry run, as a pull request's is.

**One switch for Alimardon:** GitHub lets a workflow open pull requests only once the repository allows it (Settings, Actions, General, Workflow permissions, "Allow GitHub Actions to create and approve pull requests"). Until then the rebuild still pushes its branch and runs CI, and its run summary links the page where the pull request is opened by hand with one button.

## Reading the package lists and the bills of materials

Every image comes with two files, in the release and in each CI run's artefact `edel-images`:

| File | What it holds | Use it to |
|---|---|---|
| `IMAGE.packages` | One installed package a line, `name-version`, for example `musl-1.2.5-r10` | Compare two releases with `diff`, or check one package with `grep '^openssl-' IMAGE.packages` |
| `IMAGE.cdx.json` | The same packages as a CycloneDX 1.6 bill of materials: each one's version, licence, `purl` (`pkg:apk/alpine/NAME@VERSION`) and apk origin (its source package) | Feed a scanner that reads CycloneDX, or find which source package an installed one comes from |

Alpine's advisories name source packages (`openssl`), while images install the packages built from them (`libcrypto3`, `libssl3`); the bill of materials' origin joins the two. An apk version ends in `-rN`, the package's own build number: `openssl-3.5.8-r0` is newer than `openssl-3.5.1-r0`, and `-r1` is a rebuild of the same version, often for a fix.

## Principles check

- **Reliable:** fixes are built and tested by the same full run as any change before they reach a release, and a release goes out only through its usual checklist; nothing reaches a machine untested.
- **Simple:** one small workflow and one script reuse the build, the tests and the release path that exist; the package diff is plain text in the pull request.
- **Efficient:** one light job a month, and the one full run a change gets anyway, within the shared limit of CI jobs at once.
- Traded off: a fix lands in a release once a month unless someone starts the rebuild sooner; a daily rebuild would cost a full CI run each day for little gain.
