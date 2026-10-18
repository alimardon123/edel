//! `edel report --battery MINUTES` (roadmap M7.16a): what drains a laptop's
//! battery and keeps its processor awake, measured over MINUTES with the
//! computer left alone and unplugged. It gives the watts drawn on battery,
//! how much of the time the processor spent resting in each state, which
//! processes woke it most, the share of the time the platform spent in its
//! deepest sleep (S0ix), and how the last suspend went. A voluntary context
//! switch is a thread giving up the processor, mostly to wait for something,
//! so counting them is a close proxy for wakeups (a preemption is not
//! counted). Everything comes from `/sys` and `/proc`, so no tool is added.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, bail};
use edel::battery_care::{self, POWER_SUPPLY};
use edel::i18n::trf;
use serde::Serialize;

/// The shortest measurement: the battery's charge is updated only every
/// minute or so, so a shorter one shows nothing.
pub const MIN_MINUTES: u64 = 5;

/// The processors' folders, with each one's idle states.
const CPU: &str = "/sys/devices/system/cpu";

/// The power settings and the suspend statistics.
const POWER: &str = "/sys/power";

/// The wakeup sources the kernel counts.
const WAKEUP: &str = "/sys/class/wakeup";

/// The processes, their threads and the interrupt counts.
const PROC: &str = "/proc";

/// How many processes the wakers list names.
const WAKERS: usize = 10;

/// How many wakeup sources the sleep section names.
const SOURCES: usize = 5;

/// One battery as the kernel shows it at one moment.
#[derive(Debug, Default)]
struct Cell {
    name: String,
    status: String,
    /// The energy left in µWh, from `energy_now`.
    energy_uwh: Option<u64>,
    /// The charge left in µAh, for a battery that shows no energy.
    charge_uah: Option<u64>,
    /// The design voltage in µV, which both samples share.
    voltage_design_uv: Option<u64>,
    voltage_now_uv: Option<u64>,
    /// The power drawn now in µW.
    power_uw: Option<u64>,
    capacity_percent: Option<u32>,
    /// The full charge now as a share of the design full charge.
    health_percent: Option<u32>,
    cycles: Option<u32>,
}

/// What one moment of the computer shows.
#[derive(Debug, Default)]
struct Sample {
    batteries: Vec<Cell>,
    /// Whether a charger is connected.
    on_mains: bool,
    /// Each processor state's name with its total time in µs over every
    /// processor, in the order cpu0 lists them.
    idle: Vec<(String, u64)>,
    cpus: u32,
    /// The time the platform spent in S0ix, in µs.
    s0ix_us: Option<u64>,
    /// Interrupts since boot, over every processor.
    interrupts: u64,
    /// Voluntary context switches since boot, by pid, with the command name.
    switches: BTreeMap<u32, (String, u64)>,
}

/// The `[battery]` table of `edel report --battery`.
#[derive(Debug, Default, Serialize)]
pub struct Battery {
    pub minutes: u64,
    pub plugged_in: bool,
    /// Watts drawn from the batteries over the measurement, two decimals;
    /// none when they could not be measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watts: Option<f64>,
    /// What the numbers mean, in plain words.
    pub note: String,
    /// The share of the time spent in the platform's deepest sleep (S0ix),
    /// one decimal; none when the platform keeps no count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s0ix_percent: Option<f64>,
    /// Interrupts per second over every processor, one decimal.
    pub interrupts_per_second: f64,
    pub batteries: Vec<BatteryLine>,
    /// The share of the time each processor state held, over all processors.
    pub cpu_rest: Vec<Rest>,
    /// The processes that woke the processor most, the busiest first.
    pub wakers: Vec<Waker>,
    pub sleep: Sleep,
}

/// One battery's charge and drain.
#[derive(Debug, PartialEq, Serialize)]
pub struct BatteryLine {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_wh: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_wh: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watts: Option<f64>,
    /// The power the battery reports drawing at the end, in watts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_now_watts: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_percent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_percent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycles: Option<u32>,
}

/// The share of the time one processor state held.
#[derive(Debug, PartialEq, Serialize)]
pub struct Rest {
    pub state: String,
    pub percent: f64,
}

/// A process that woke the processor, by its voluntary context switches.
#[derive(Debug, PartialEq, Serialize)]
pub struct Waker {
    pub name: String,
    pub pid: u32,
    pub per_second: f64,
}

/// How the last suspend went and what woke the computer from it.
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Sleep {
    /// The sleep the system uses, the bracketed entry of `mem_sleep`; empty
    /// when the file is missing.
    pub mode: String,
    pub successes: u64,
    pub failures: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_failed_device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_hw_sleep_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_hw_sleep_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s0ix_seconds_total: Option<f64>,
    /// The interrupt that woke the computer and its devices, such as `9 acpi`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub woke_by: String,
    /// The wakeup sources that woke the computer most, since boot.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wakeup_sources: Vec<Source>,
}

/// A wakeup source and how often it woke the computer.
#[derive(Debug, PartialEq, Serialize)]
pub struct Source {
    pub name: String,
    pub wakeups: u64,
}

/// `value` rounded to `places` decimal places.
fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

/// A file's text without its line end, or empty when it cannot be read (a
/// read that fails with ENODATA, as `pm_wakeup_irq` does when there is no
/// interrupt, is empty too).
fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name))
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// A file's number, or None when it is missing or not a number.
fn number<T: std::str::FromStr>(dir: &Path, name: &str) -> Option<T> {
    read(dir, name).parse().ok()
}

/// A time in µs, as seconds with two decimals.
fn seconds(dir: &Path, name: &str) -> Option<f64> {
    number::<u64>(dir, name).map(|us| round(us as f64 / 1e6, 2))
}

/// The folders in `dir` named `prefix` and a number, in number order.
fn numbered(dir: &Path, prefix: &str) -> Vec<(u32, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(u32, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let index = entry
                .file_name()
                .to_str()?
                .strip_prefix(prefix)?
                .parse()
                .ok()?;
            Some((index, entry.path()))
        })
        .collect();
    found.sort();
    found
}

/// The percent `part` is of `whole`, or None when either is missing or
/// `whole` is zero.
fn percent_of(part: Option<u64>, whole: Option<u64>) -> Option<u32> {
    let (part, whole) = (part?, whole?);
    (whole > 0).then(|| (part as f64 / whole as f64 * 100.0).round() as u32)
}

/// One battery's folder as the kernel shows it.
fn cell(dir: &Path) -> Cell {
    let energy_full = number::<u64>(dir, "energy_full");
    let energy_design = number::<u64>(dir, "energy_full_design");
    let charge_full = number::<u64>(dir, "charge_full");
    let charge_design = number::<u64>(dir, "charge_full_design");
    Cell {
        name: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        status: read(dir, "status"),
        energy_uwh: number(dir, "energy_now"),
        charge_uah: number(dir, "charge_now"),
        voltage_design_uv: number(dir, "voltage_min_design"),
        voltage_now_uv: number(dir, "voltage_now"),
        power_uw: number(dir, "power_now"),
        capacity_percent: number(dir, "capacity"),
        health_percent: percent_of(energy_full, energy_design)
            .or_else(|| percent_of(charge_full, charge_design)),
        cycles: number::<u32>(dir, "cycle_count").filter(|&count| count > 0),
    }
}

/// The voltage a battery's energy is worked out with: its design voltage,
/// else its voltage now.
fn volt_of(cell: &Cell) -> Option<u64> {
    cell.voltage_design_uv.or(cell.voltage_now_uv)
}

/// The energy a battery holds in µWh: `energy_now`, else its charge times
/// `volt_uv`, which the caller picks once for both samples.
fn energy_uwh(cell: &Cell, volt_uv: Option<u64>) -> Option<u64> {
    cell.energy_uwh
        .or_else(|| Some(cell.charge_uah? * volt_uv? / 1_000_000))
}

/// The voluntary context switches in one thread's `status` text.
fn status_switches(text: &str) -> u64 {
    text.lines()
        .find_map(|line| line.strip_prefix("voluntary_ctxt_switches:"))
        .and_then(|count| count.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// The voluntary context switches of every process under `proc`, by pid.
fn switches(proc: &Path) -> BTreeMap<u32, (String, u64)> {
    let mut found = BTreeMap::new();
    let Ok(entries) = fs::read_dir(proc) else {
        return found;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let dir = entry.path();
        let Ok(tasks) = fs::read_dir(dir.join("task")) else {
            continue;
        };
        let total: u64 = tasks
            .flatten()
            .map(|task| {
                fs::read_to_string(task.path().join("status"))
                    .map(|text| status_switches(&text))
                    .unwrap_or(0)
            })
            .sum();
        found.insert(pid, (read(&dir, "comm"), total));
    }
    found
}

/// The interrupts since boot, from the first number of `intr` in
/// `proc/stat`.
fn interrupts(proc: &Path) -> u64 {
    fs::read_to_string(proc.join("stat"))
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("intr "))
        .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Reads the batteries under `power_supply`, the processors under `cpu` and
/// the processes under `proc` at this moment; a missing file is empty or
/// zero, never an error.
fn sample(power_supply: &Path, cpu: &Path, proc: &Path) -> Sample {
    let cpus = numbered(cpu, "cpu");
    let mut idle: Vec<(String, u64)> = Vec::new();
    for (_, dir) in &cpus {
        for (_, state) in numbered(&dir.join("cpuidle"), "state") {
            let name = read(&state, "name");
            let time = number::<u64>(&state, "time").unwrap_or(0);
            match idle.iter_mut().find(|(known, _)| *known == name) {
                Some((_, total)) => *total += time,
                None => idle.push((name, time)),
            }
        }
    }
    Sample {
        batteries: battery_care::batteries(power_supply)
            .iter()
            .map(|dir| cell(dir))
            .collect(),
        on_mains: battery_care::plugged_in(power_supply),
        idle,
        cpus: u32::try_from(cpus.len()).unwrap_or(u32::MAX),
        s0ix_us: number(&cpu.join("cpuidle"), "low_power_idle_system_residency_us"),
        interrupts: interrupts(proc),
        switches: switches(proc),
    }
}

/// The note in plain words: why no drain was measured, or the drain.
fn note(batteries: usize, plugged_in: bool, minutes: u64, watts: Option<f64>) -> String {
    if batteries == 0 {
        return trf(
            "this computer has no battery, so only the processor's rest and wakeups are measured",
            &[],
        );
    }
    if plugged_in {
        return trf(
            "it was plugged in, so no drain was measured; unplug it and measure again",
            &[],
        );
    }
    match watts {
        Some(watts) => trf(
            "it drew {watts} W on battery over {minutes} minutes",
            &[
                ("watts", &format!("{watts:.2}")),
                ("minutes", &minutes.to_string()),
            ],
        ),
        None => trf(
            "its charge could not be read, so no drain was measured",
            &[],
        ),
    }
}

/// The measurement between two samples taken `seconds` apart. Pure, so it
/// is tested on made-up samples. The sleep section is left empty; `measure`
/// fills it.
fn compare(before: &Sample, after: &Sample, seconds: f64) -> Battery {
    let hours = seconds / 3600.0;
    let lines: Vec<BatteryLine> = after
        .batteries
        .iter()
        .map(|now| {
            let was = before.batteries.iter().find(|cell| cell.name == now.name);
            let volt = was.and_then(volt_of).or_else(|| volt_of(now));
            let start = was.and_then(|cell| energy_uwh(cell, volt));
            let end = energy_uwh(now, volt);
            let watts = match (start, end) {
                (Some(start), Some(end))
                    if now.status == "Discharging" && start > end && hours > 0.0 =>
                {
                    Some(round((start - end) as f64 / 1e6 / hours, 2))
                }
                _ => None,
            };
            BatteryLine {
                name: now.name.clone(),
                status: now.status.clone(),
                start_wh: start.map(|uwh| round(uwh as f64 / 1e6, 2)),
                end_wh: end.map(|uwh| round(uwh as f64 / 1e6, 2)),
                watts,
                power_now_watts: now.power_uw.map(|uw| round(uw as f64 / 1e6, 2)),
                capacity_percent: now.capacity_percent,
                health_percent: now.health_percent,
                cycles: now.cycles,
            }
        })
        .collect();
    let plugged_in = before.on_mains
        || after.on_mains
        || before
            .batteries
            .iter()
            .chain(after.batteries.iter())
            .any(|cell| matches!(cell.status.as_str(), "Charging" | "Full" | "Not charging"));
    let drains: Vec<f64> = lines.iter().filter_map(|line| line.watts).collect();
    let watts = (!drains.is_empty()).then(|| round(drains.iter().sum(), 2));
    let minutes = (seconds / 60.0).round() as u64;

    let cpu_rest = if after.cpus == 0 || seconds <= 0.0 {
        Vec::new()
    } else {
        let all = seconds * 1e6 * f64::from(after.cpus);
        after
            .idle
            .iter()
            .map(|(state, now)| {
                let was = before
                    .idle
                    .iter()
                    .find(|(known, _)| known == state)
                    .map_or(0, |(_, time)| *time);
                Rest {
                    state: state.clone(),
                    percent: round(now.saturating_sub(was) as f64 / all * 100.0, 1),
                }
            })
            .collect()
    };

    let mut wakers: Vec<Waker> = after
        .switches
        .iter()
        .filter_map(|(pid, (name, now))| {
            let (_, was) = before.switches.get(pid)?;
            let delta = now.saturating_sub(*was);
            (delta > 0 && seconds > 0.0).then(|| Waker {
                name: name.clone(),
                pid: *pid,
                per_second: round(delta as f64 / seconds, 2),
            })
        })
        .collect();
    wakers.sort_by(|a, b| {
        b.per_second
            .total_cmp(&a.per_second)
            .then_with(|| a.pid.cmp(&b.pid))
    });
    wakers.truncate(WAKERS);

    let s0ix_percent = match (before.s0ix_us, after.s0ix_us) {
        (Some(was), Some(now)) if seconds > 0.0 => Some(round(
            now.saturating_sub(was) as f64 / (seconds * 1e6) * 100.0,
            1,
        )),
        _ => None,
    };
    let interrupts_per_second = if seconds > 0.0 {
        round(
            after.interrupts.saturating_sub(before.interrupts) as f64 / seconds,
            1,
        )
    } else {
        0.0
    };

    Battery {
        minutes,
        plugged_in,
        watts,
        note: note(after.batteries.len(), plugged_in, minutes, watts),
        s0ix_percent,
        interrupts_per_second,
        batteries: lines,
        cpu_rest,
        wakers,
        sleep: Sleep::default(),
    }
}

/// The interrupt `irq` and the devices its line of `proc/interrupts` names,
/// such as `9 acpi`: after the counts come the chip and the trigger, then
/// the devices.
fn woke_by(proc: &Path, irq: u32) -> String {
    let text = fs::read_to_string(proc.join("interrupts")).unwrap_or_default();
    let devices = text
        .lines()
        .find_map(|line| {
            let (number, rest) = line.split_once(':')?;
            (number.trim() == irq.to_string()).then_some(rest)
        })
        .map(|rest| {
            rest.split_whitespace()
                .skip_while(|word| word.parse::<u64>().is_ok())
                .skip(2)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if devices.is_empty() {
        irq.to_string()
    } else {
        format!("{irq} {devices}")
    }
}

/// How the last suspend went, read from `power` (`mem_sleep`,
/// `suspend_stats`, `pm_wakeup_irq`), `cpu` (S0ix), `wakeup` (the wakeup
/// sources) and `proc` (the interrupt's devices). Missing files are empty
/// or zero.
fn sleep(power: &Path, cpu: &Path, wakeup: &Path, proc: &Path) -> Sleep {
    let stats = power.join("suspend_stats");
    let mode = read(power, "mem_sleep")
        .split_whitespace()
        .find_map(|word| {
            word.strip_prefix('[')?
                .strip_suffix(']')
                .map(str::to_string)
        })
        .unwrap_or_default();
    let mut wakeup_sources: Vec<Source> = fs::read_dir(wakeup)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let wakeups = number::<u64>(&dir, "wakeup_count")?;
            (wakeups > 0).then(|| Source {
                name: read(&dir, "name"),
                wakeups,
            })
        })
        .collect();
    wakeup_sources.sort_by(|a, b| b.wakeups.cmp(&a.wakeups).then_with(|| a.name.cmp(&b.name)));
    wakeup_sources.truncate(SOURCES);
    Sleep {
        mode,
        successes: number(&stats, "success").unwrap_or(0),
        failures: number(&stats, "fail").unwrap_or(0),
        last_failed_device: read(&stats, "last_failed_dev"),
        last_hw_sleep_seconds: seconds(&stats, "last_hw_sleep"),
        total_hw_sleep_seconds: seconds(&stats, "total_hw_sleep"),
        s0ix_seconds_total: seconds(&cpu.join("cpuidle"), "low_power_idle_system_residency_us"),
        woke_by: number::<u32>(power, "pm_wakeup_irq")
            .map(|irq| woke_by(proc, irq))
            .unwrap_or_default(),
        wakeup_sources,
    }
}

/// Measures the battery for `minutes` with the computer left alone and
/// unplugged, and says so on standard error. Refuses a shorter measurement.
pub fn measure(minutes: u64) -> Result<Battery> {
    if minutes < MIN_MINUTES {
        bail!(
            "{}",
            trf(
                "measure for {min} minutes or more: the battery's charge is updated only every minute or so",
                &[("min", &MIN_MINUTES.to_string())],
            )
        );
    }
    eprintln!(
        "edel report: {}",
        trf(
            "measuring the battery for {minutes} minutes; leave the computer alone, unplugged, with its screen on",
            &[("minutes", &minutes.to_string())],
        )
    );
    let before = sample(Path::new(POWER_SUPPLY), Path::new(CPU), Path::new(PROC));
    let seconds = minutes * 60;
    std::thread::sleep(Duration::from_secs(seconds));
    let after = sample(Path::new(POWER_SUPPLY), Path::new(CPU), Path::new(PROC));
    let mut battery = compare(&before, &after, seconds as f64);
    battery.sleep = sleep(
        Path::new(POWER),
        Path::new(CPU),
        Path::new(WAKEUP),
        Path::new(PROC),
    );
    Ok(battery)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for `test` under the temp directory, named after the
    /// test and this process, which the test removes when it is done.
    fn tree(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("edel-battery-report-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file `name` under `dir` holding `text`, its folders made.
    fn file(dir: &Path, name: &str, text: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), format!("{text}\n")).unwrap();
    }

    #[test]
    fn reads_energy_and_falls_back_to_charge_times_voltage() {
        let dir = tree("energy");
        let power = dir.join("power_supply");
        let bat0 = power.join("BAT0");
        let bat1 = power.join("BAT1");
        file(&bat0, "type", "Battery");
        file(&bat0, "energy_now", "30000000");
        file(&bat0, "status", "Discharging");
        file(&bat1, "type", "Battery");
        file(&bat1, "charge_now", "2000000");
        file(&bat1, "voltage_min_design", "12000000");
        file(&bat1, "voltage_now", "11000000");
        let before = sample(&power, &dir.join("cpu"), &dir.join("proc"));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(before.batteries.len(), 2);
        assert_eq!(before.batteries[0].energy_uwh, Some(30_000_000));
        assert_eq!(energy_uwh(&before.batteries[0], None), Some(30_000_000));
        // 2 Ah at the design voltage of 12 V is 24 Wh.
        let bat1 = &before.batteries[1];
        assert_eq!(volt_of(bat1), Some(12_000_000));
        assert_eq!(energy_uwh(bat1, volt_of(bat1)), Some(24_000_000));
    }

    #[test]
    fn skips_a_device_battery_and_counts_the_charger() {
        let dir = tree("device");
        let power = dir.join("power_supply");
        file(&power.join("mouse"), "type", "Battery");
        file(&power.join("mouse"), "scope", "Device");
        file(&power.join("AC"), "type", "Mains");
        file(&power.join("AC"), "online", "1");
        let found = sample(&power, &dir.join("cpu"), &dir.join("proc"));
        let _ = fs::remove_dir_all(&dir);
        assert!(found.batteries.is_empty(), "a mouse is not the computer's");
        assert!(found.on_mains);
    }

    #[test]
    fn compare_gives_watts_on_battery_and_none_plugged_in() {
        let battery = |status: &str, energy: u64| Cell {
            name: "BAT0".into(),
            status: status.into(),
            energy_uwh: Some(energy),
            ..Default::default()
        };
        let sample_of = |status: &str, energy: u64| Sample {
            batteries: vec![battery(status, energy)],
            cpus: 2,
            ..Default::default()
        };
        // 1 Wh less in 6 minutes is 10 W.
        let drained = compare(
            &sample_of("Discharging", 50_000_000),
            &sample_of("Discharging", 49_000_000),
            360.0,
        );
        assert_eq!(drained.watts, Some(10.0));
        assert_eq!(drained.minutes, 6);
        assert!(!drained.plugged_in);
        assert_eq!(drained.note, "it drew 10.00 W on battery over 6 minutes");
        let charging = compare(
            &sample_of("Charging", 50_000_000),
            &sample_of("Charging", 51_000_000),
            360.0,
        );
        assert_eq!(charging.watts, None);
        assert!(charging.plugged_in);
        assert_eq!(
            charging.note,
            "it was plugged in, so no drain was measured; unplug it and measure again"
        );
    }

    #[test]
    fn rest_percent_is_over_every_processor() {
        let state = |time: u64| Sample {
            cpus: 2,
            idle: vec![("C1".into(), time)],
            ..Default::default()
        };
        // 1.2 s of rest in C1 over two processors for 1 s is 60 percent.
        let battery = compare(&state(0), &state(1_200_000), 1.0);
        assert_eq!(
            battery.cpu_rest,
            [Rest {
                state: "C1".into(),
                percent: 60.0
            }]
        );
    }

    #[test]
    fn wakers_are_the_busiest_voluntary_switchers_cut_at_ten() {
        let process = |pid: u32, count: u64| (format!("p{pid}"), count);
        let before = Sample {
            switches: (1..=12).map(|pid| (pid, process(pid, 0))).collect(),
            ..Default::default()
        };
        let after = Sample {
            switches: (1..=12)
                .map(|pid| (pid, process(pid, u64::from(pid) * 10)))
                .collect(),
            ..Default::default()
        };
        let battery = compare(&before, &after, 10.0);
        assert_eq!(battery.wakers.len(), WAKERS);
        assert_eq!(battery.wakers[0].pid, 12);
        assert_eq!(battery.wakers[0].name, "p12");
        assert_eq!(battery.wakers[0].per_second, 12.0);
        assert!(
            battery
                .wakers
                .windows(2)
                .all(|pair| pair[0].per_second >= pair[1].per_second)
        );
    }

    #[test]
    fn counts_only_voluntary_switches() {
        let status =
            "Name:\tfirefox\nvoluntary_ctxt_switches:\t40\nnonvoluntary_ctxt_switches:\t9\n";
        assert_eq!(status_switches(status), 40);
        assert_eq!(status_switches("Name:\tx\n"), 0);
    }

    #[test]
    fn sleep_reads_the_bracketed_mode_and_the_top_sources() {
        let dir = tree("sleep");
        let power = dir.join("power");
        let cpu = dir.join("cpu");
        let wakeup = dir.join("wakeup");
        let proc = dir.join("proc");
        file(&power, "mem_sleep", "s2idle [deep]");
        file(&power.join("suspend_stats"), "success", "3");
        file(&power.join("suspend_stats"), "fail", "1");
        file(
            &power.join("suspend_stats"),
            "last_failed_dev",
            "pci0000:00",
        );
        file(&power.join("suspend_stats"), "last_hw_sleep", "2500000");
        file(&power, "pm_wakeup_irq", "9");
        file(&proc, "interrupts", " 9:  0  0  IO-APIC  9-fasteoi   acpi");
        file(
            &cpu.join("cpuidle"),
            "low_power_idle_system_residency_us",
            "7000000",
        );
        for (name, count) in [
            ("a", "5"),
            ("b", "3"),
            ("c", "0"),
            ("d", "9"),
            ("e", "1"),
            ("f", "2"),
            ("g", "8"),
        ] {
            file(&wakeup.join(name), "name", name);
            file(&wakeup.join(name), "wakeup_count", count);
        }
        let found = sleep(&power, &cpu, &wakeup, &proc);
        let missing = sleep(
            &dir.join("none"),
            &dir.join("none"),
            &dir.join("none"),
            &proc,
        );
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(found.mode, "deep");
        assert_eq!((found.successes, found.failures), (3, 1));
        assert_eq!(found.last_failed_device, "pci0000:00");
        assert_eq!(found.last_hw_sleep_seconds, Some(2.5));
        assert_eq!(found.s0ix_seconds_total, Some(7.0));
        assert_eq!(found.woke_by, "9 acpi");
        let names: Vec<_> = found
            .wakeup_sources
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["d", "g", "a", "b", "f"]);
        assert_eq!(missing.mode, "", "a missing mem_sleep is empty");
        assert_eq!(missing.woke_by, "");
        assert!(missing.wakeup_sources.is_empty());
    }

    #[test]
    fn refuses_a_measurement_under_five_minutes() {
        let err = measure(4).unwrap_err().to_string();
        assert!(err.starts_with("measure for 5 minutes or more"), "{err}");
    }
}
