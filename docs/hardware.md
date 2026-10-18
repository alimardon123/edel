<!-- Written by a cargo test from docs/hardware/*.toml, crates/edel/src/hardware_table.rs; never edit it by hand: add a report and run EDEL_WRITE_DOCS=1 cargo test -p edel hardware_table. -->

# Hardware reports

Each machine below sent `edel report` (roadmap M8.1): the stick writes it to its EFI system partition as `EFI/edel/report.toml` at every start, and on an installed machine `edel report > report.toml` prints it. Copy it off the stick before setting the answers: the stick writes it again at every start. Before attaching it to an issue, set each `[[basics]]` row's `works` to "yes" or "no". The report holds no personal files.

| Machine | Release | Start | Memory at start | Frame render p99 | Basics that work |
|---|---|---|---|---|---|
| QEMU Standard PC (Q35 + ICH9, 2009) ([report](hardware/qemu-standard-pc.toml)) | 2026.10.1554 | 5.85 s | 67 MiB | 3.40 ms | 11 of 12 answered |

## Basics that do not work

- QEMU Standard PC (Q35 + ICH9, 2009): Camera
