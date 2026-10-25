# Spike: app runtimes that keep apps working for years

**Date:** 2026-10-25 (roadmap M10.9 and M10.10)
**Asked by:** Alimardon, on 2026-10-25: test now the ways apps could be "independent" of the system for 10 to 20 years, on old and new Edel OS alike, while staying "much more faster, lightweight, just like Alpine".

## Verdict

**All three ways run, an older app works on a newer base and a newer app on an older one, and a Flatpak runtime built from Alpine's packages is the best default for our own and the default apps.** It downloads about a quarter of what Flathub's does, takes about a fifth of the disk, and opens as fast, while keeping Flatpak's one format, sandbox, sharing and updates. Apps that carry everything (M10.9) open fastest but copy GTK into every app. Flathub stays for apps built only for glibc (Steam, Chrome, VS Code). Choosing the Alpine runtime changes ADR-003, so it waits for Alimardon's word.

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

- **A runtime from Alpine is the light Flatpak.** One download serves every app on it, like Flathub's, and it is about a quarter the size, because musl, Alpine's small packages and no second copy of the graphics extension add up. Apps on it are a few MB each.
- **Because it is musl like the base, it can take the machine's own graphics drivers,** with the same "newer copy wins" rule the bundles use. An old runtime then knows new GPUs without a rebuilt graphics extension (M10.2). Flathub's glibc runtimes cannot use our musl drivers. Taking them from the base loosens "apps never link against the base" (ADR-003, ADR-005) to "apps link only against the frozen part", which waits for Alimardon's word; until then the runtime carries its own mesa, as measured here.
- **Bundles suit command-line tools** (about 2 MB each) and single apps, but every GTK app would carry about 200 MB of its own GTK, so ten apps take about 2 GB. They are a second app format beside Flatpak, which Simple argues against.
- **Ten years and more:** none of the three is held up by the base. Bundles and our runtime rest on musl's promise never to break an older program and on the kernel's; Flathub's on glibc inside its runtime. What ages is the graphics driver inside an old runtime, which our runtime avoids by taking the base's.
- **Not measured yet:** runtimes built from Debian's or Fedora's packages (M10.10's wider question), a real GPU (these ran on mesa's software renderer in containers, so times compare the ways but are not what a laptop shows), and the yearly work to build and update a runtime.

## Principles check

- **Reliable:** every way was run on an older and a newer base before anything changes; nothing ships from the spike, and the base and the updater are untouched.
- **Instant:** the Alpine runtime and the bundles open faster than Flathub's app here; the gap is measured, not assumed.
- **Simple:** the recommendation keeps one app format, Flatpak, and adds no part to the base.
- **Efficient:** a quarter of Flathub's download and a fifth of its disk for the same app.
- **Traded off:** a yearly runtime of our own to build and keep secure, and, if Alimardon agrees, apps loading the base's graphics drivers.
