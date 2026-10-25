# Spike: app runtimes that keep apps working for years

**Date:** 2026-10-25 (roadmap M10.9 and M10.10), with the review and fairer tests the same day
**Asked by:** Alimardon, on 2026-10-25: test now the ways apps could be "independent" of the system for 10 to 20 years, on old and new Edel OS alike, while staying "much more faster, lightweight, just like Alpine".

## Verdict

**All three ways run, an older app works on a newer base and a newer app on an older one, but Flathub stays the one app format for now (ADR-003 unchanged).** A Flatpak runtime built from Alpine's packages measured lighter and quicker to open, and an old one, built without a graphics driver, draws with a newer machine's own driver. But an outside review (2026-10-25, below) weighed what the numbers leave out: GTK sits in Alpine's community repository, which gets fixes for about six months, so a runtime of our own means patching it ourselves for years; third-party apps target Flathub's runtimes anyway; and Flathub's runtime carries far more. The seam worth keeping is the machine lending its own driver to old runtimes. A runtime of our own comes back only when someone can keep it secure, with the hours a year written down.

## What was measured

Three ways to give an app what it needs, each measured with GNOME Calculator (a GTK 4 and libadwaita app):

1. **Flathub (today, ADR-003):** the app from Flathub on `org.gnome.Platform`, with Flathub's graphics extension.
2. **Apps that carry everything (M10.9):** the app and every library it needs, from Alpine's packages, in one folder; the machine gives only the frozen part: the kernel, musl's loader, the graphics drivers (mesa) and the standard protocols. It runs in a bubblewrap sandbox like Flatpak's, with its own folder for its data kept between runs, as Flatpak keeps `~/.var/app/ID`. A library both the app and the drivers carry is taken from whichever copy is newer, as Valve's Steam runtime does. A GPU app (es2gears) and two command-line tools (jq, ripgrep) were bundled the same way.
3. **A Flatpak runtime built from Alpine's packages (M10.10):** `org.edel.Platform//BRANCH` made from that branch's musl, GTK 4, libadwaita, mesa and fonts; the calculator on it made from the eleven packages it adds, `/usr` becoming `/app`.

The newer base is the Alpine branch `features/base.toml` names, the older one the branch a year before it. Every bundle was built from both and run on both; both runtimes ran on the newer base. [`ci/runtime-spike/all.sh`](https://github.com/alimardon123/edel/tree/main/ci/runtime-spike) does it all in Docker, and the workflow "App runtime spike" runs it whenever its files change.

## Results

Measured in a cloud session on 2026-10-25, the newer base Alpine 3.24 (GTK 4.22, mesa 26.1, musl 1.2.6) and the older 3.22 (GTK 4.18, mesa 25.1, musl 1.2.5); "later" is the best of the second and third runs, memory the same run's. Every app drew or ran in every row.

| App | Way | Built from | Runs on | Download | On disk | First frame, first run | First frame, later | Memory |
|---|---|---|---|---|---|---|---|---|
| Calculator | Flathub | Flathub | 3.24 | 721 MB | 2514 MB | 1500 ms | 680 to 1120 ms | 224 MB |
| Calculator | Alpine runtime | 3.24 | 3.24 | 186 MB, then 9 MB an app | 484 MB | 2340 ms | 380 to 410 ms | 221 MB |
| Calculator | Alpine runtime | 3.22 | 3.24 | 190 MB, then 9 MB an app | 525 MB | 2190 ms | 430 to 460 ms | 229 MB |
| Calculator | bundle | 3.24 | 3.24 | 84 MB | 229 MB | 2610 ms | 430 ms | 196 MB |
| Calculator | bundle | 3.22 | 3.24 | 71 MB | 187 MB | 2500 ms | 370 ms | 195 MB |
| Calculator | bundle | 3.24 | 3.22 | 84 MB | 229 MB | 2910 ms | 400 ms | 204 MB |
| Calculator | bundle | 3.22 | 3.22 | 71 MB | 187 MB | 2480 ms | 450 ms | 201 MB |
| es2gears (GPU) | bundle | 3.22 | 3.24 | 19 MB | 59 MB | 410 ms | 80 ms | 119 MB |
| es2gears (GPU) | bundle | 3.24 | 3.22 | 19 MB | 60 MB | 310 ms | 80 ms | 126 MB |
| jq and ripgrep | bundle | either | either | 2 MB | 5 MB | 160 to 210 ms for both commands | | |

The machine's graphics part the bundles take (mesa, libdrm, LLVM and what they load) is 240 MB on 3.24 and 319 MB on 3.22; it is already on every desktop and counts for no app. An older bundle on the newer base took four libraries from the drivers' newer copies (`libwayland-client`, `libstdc++`, `libffi`, `libmd`); without that rule its graphics failed (`wl_display_dispatch_queue_timeout: symbol not found`). Flathub's later starts varied from 680 to 1120 ms between two runs of the spike.

Download is the gzip of a bundle, the compressed objects of our Flatpak repository, and the bytes received for Flathub. Memory is the app's proportional set size 3 s after its first frame. The first run is slower in every way because the graphics driver compiles its shaders and keeps them in the app's data folder.

## What it means

The first reading, before the review below weighed it:

- **A runtime from Alpine is the light Flatpak.** One download serves every app on it, like Flathub's, and it is about a quarter the size, because musl, Alpine's small packages and no second copy of the graphics extension add up. Apps on it are a few MB each.
- **Because it is musl like the base, it can take the machine's own graphics drivers,** with the same "newer copy wins" rule the bundles use. An old runtime then knows new GPUs without a rebuilt graphics extension (M10.2). Flathub's glibc runtimes cannot use our musl drivers. Taking them from the base loosens "apps never link against the base" (ADR-003, ADR-005) to "apps link only against the frozen part", which waits for Alimardon's word; until then the runtime carries its own mesa, as measured here.
- **Bundles suit command-line tools** (about 2 MB each) and single apps, but every GTK app would carry about 200 MB of its own GTK, so ten apps take about 2 GB. They are a second app format beside Flatpak, which Simple argues against.
- **Ten years and more:** none of the three is held up by the base. Bundles and our runtime rest on musl's promise never to break an older program and on the kernel's; Flathub's on glibc inside its runtime. What ages is the graphics driver inside an old runtime, which our runtime avoids by taking the base's.
- **Not measured yet:** runtimes built from Debian's or Fedora's packages (M10.10's wider question), a real GPU (these ran on mesa's software renderer in containers, so times compare the ways but are not what a laptop shows), and the yearly work to build and update a runtime.

## The review and the fairer tests (2026-10-25)

Alimardon asked for an independent review of the first verdict (a Fable advisor, working offline from this page and the ADRs). It found the recommendation "half right, and too early": the runtime's upkeep (GTK and glib fixes for every runtime still in use, by one person), the size comparison (Flathub's GNOME platform holds media, spelling and translations ours did not), single-app memory that cannot show sharing, the software renderer, and default apps that need no runtime at all. It asked for three more tests, all in `ci/runtime-spike/`:

1. **Default apps inside the system** (Alpine packages beside Settings, which already carries GTK 4 and libadwaita): Calculator, Text Editor, Clocks and Loupe together add 28 MB to a slot and 10 MB to each update; the store, the files app and the editor of today's defaults (Bazaar, Portfolio, Text Editor) add 191 MB and 70 MB, and Firefox 306 MB and 117 MB. Small apps would be cheap there, the defaults as chosen would not, and each update would carry them every month.
2. **An old runtime on a newer machine's driver** (`host-gl.sh`): runtimes from Alpine 3.18 (2023, musl 1.2.4) and 3.22, built with no graphics driver, both drew with the 3.24 machine's mesa 26.1, its `libgallium` loaded from the machine, ten libraries (3.18) or four (3.22) taken from the driver's newer copies. The feared mismatch, a newer driver needing a newer musl than the runtime's, did not happen with this mesa; started by the machine's own musl loader instead, it works too, the way out if it ever does. So the 10 year promise for GPUs need not rebuild a graphics extension for every old runtime (M10.2), for a musl runtime.
3. **Flathub, fairly** (`fair.sh`, ten starts, three apps at once): the calculator opened in a median of 730 ms from Flathub and 430 ms from the Alpine runtime, steady across ten runs; Calculator, Text Editor and Clocks open together took 463 MB and 455 MB, the same; the first start is 1.5 s against 2.3 s, Flathub ahead. On disk Flathub took 2.6 GB (its GNOME platform 1.1 GB, and the graphics extension twice, 470 MB each, the second for its codecs), the Alpine runtime 0.5 GB; to download 753 MB against 198 MB.

These ran in the same containers, on the software renderer; a real GPU is still to be measured, on a machine Alimardon has.

## Principles check

- **Reliable:** every way was run on an older and a newer base before anything changes; nothing ships from the spike, and the base and the updater are untouched.
- **Instant:** the Alpine runtime and the bundles open faster than Flathub's app here; the gap is measured, not assumed.
- **Simple:** one app format, Flatpak and Flathub, stays; nothing is added to the base.
- **Efficient:** the tests show where Flathub's weight is (its GNOME platform, and its graphics extension twice), for when a runtime of our own is weighed again.
- **Traded off:** Flathub's larger download and disk, and its slower start in these runs, for not keeping a runtime secure ourselves.
