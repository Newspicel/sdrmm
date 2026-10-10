mod convert;
mod noise;
mod ours;
mod parse;
mod programs;
mod signals;
mod usage;

use std::{
    collections::BTreeSet,
    num::NonZero,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use anyhow::{Context, Result, anyhow, ensure};
use clap::Args;

use super::report::{self, Better, Group, SELF, Suite};
use convert::Fixture;
use noise::Tally;
use ours::FrontEnd;
use signals::{INPUT, Reference, SIGNALS, Show, Signal};

const RUNS: usize = 3;

#[derive(Args)]
pub struct Scope {
    #[arg(long)]
    pub ours: bool,
    #[arg(long, help = "Measure one signal and print it without publishing")]
    pub signal: Option<String>,
}

pub fn run(root: &Path, scope: &Scope) -> Result<()> {
    if cfg!(debug_assertions) {
        return rerun_release(root, scope);
    }
    let work = root.join("target/compare/decoders");
    std::fs::create_dir_all(&work).with_context(|| format!("create {}", work.display()))?;
    let version = report::version(root)?;
    let chosen = chosen(scope.signal.as_deref())?;
    let mut groups = Vec::new();
    for signal in chosen {
        groups.extend(measure(root, &work, &version, signal, scope.ours)?);
    }
    if scope.signal.is_some() {
        return Ok(());
    }
    let suite = Suite {
        machine: report::machine()?,
        groups,
    };
    report::publish(root, "decoders", suite, scope.ours)
}

fn chosen(id: Option<&str>) -> Result<Vec<&'static Signal>> {
    let Some(id) = id else {
        return Ok(SIGNALS.iter().collect());
    };
    let signal = SIGNALS
        .iter()
        .find(|signal| signal.id == id)
        .with_context(|| {
            let known: Vec<&str> = SIGNALS.iter().map(|signal| signal.id).collect();
            format!("unknown signal `{id}`; known: {}", known.join(", "))
        })?;
    Ok(vec![signal])
}

fn rerun_release(root: &Path, scope: &Scope) -> Result<()> {
    let status = Command::new("cargo")
        .args([
            "run",
            "--release",
            "-p",
            "xtask",
            "--",
            "compare",
            "decoders",
        ])
        .args(scope.ours.then_some("--ours"))
        .args(
            scope
                .signal
                .as_deref()
                .into_iter()
                .flat_map(|id| ["--signal", id]),
        )
        .current_dir(root)
        .status()
        .context("run cargo")?;
    ensure!(status.success(), "the release run failed");
    Ok(())
}

fn measure(
    root: &Path,
    work: &Path,
    version: &str,
    signal: &Signal,
    ours: bool,
) -> Result<Vec<Group>> {
    let clean = Fixture::load(root, signal.fixture)?;
    let fixture = prepared(&clean, signal)?;
    let front = FrontEnd::new(signal)?;
    let loops = signal.loops(fixture.seconds());
    let fed = loops as f64 * fixture.seconds();
    let mut decodes = Group::new(
        &format!("{}-decodes", signal.id),
        &format!("{} decodes", signal.name),
        "msgs",
        Better::Higher,
    );
    let mut speed = Group::new(
        &format!("{}-speed", signal.id),
        &format!("{} speed", signal.name),
        "× realtime",
        Better::Higher,
    );
    let mut decoders = Vec::new();
    let keys = ours::decode(&fixture, signal, 1)?.keys;
    let cpu = median((0..RUNS).map(|_| Ok(ours::decode(&fixture, signal, loops)?.cpu_seconds)))?;
    record(
        &mut decodes,
        &mut speed,
        SELF,
        version,
        count(signal, &keys),
        fed / cpu,
    );
    decoders.push(Measured {
        decoder: Decoder::Ours,
        name: SELF,
        version: version.to_owned(),
        clean: keys,
    });
    let references = if ours { &[][..] } else { signal.references };
    for reference in references {
        let ready = programs::prepare(root, reference.program)?;
        let inputs = Inputs::write(work, &fixture, &front, signal, reference, loops)?;
        let keys = invoke(&ready.binary, reference, &inputs.once)?.keys;
        let cpu = median(
            (0..RUNS).map(|_| Ok(invoke(&ready.binary, reference, &inputs.looped)?.cpu_seconds)),
        )?;
        let name = reference.program.name;
        record(
            &mut decodes,
            &mut speed,
            name,
            &ready.version,
            count(signal, &keys),
            fed / cpu,
        );
        decoders.push(Measured {
            decoder: Decoder::Program(reference, ready.binary),
            name,
            version: ready.version,
            clean: keys,
        });
    }
    let noise = sweep(work, &clean, &front, signal, &decoders)?;
    Ok([(decodes, signal.decodes), (speed, signal.speed)]
        .into_iter()
        .filter_map(|(group, show)| shown(group, show))
        .chain([noise])
        .collect())
}

enum Decoder {
    Ours,
    Program(&'static Reference, PathBuf),
}

struct Measured {
    decoder: Decoder,
    name: &'static str,
    version: String,
    clean: Vec<String>,
}

fn sweep(
    work: &Path,
    clean: &Fixture,
    front: &FrontEnd,
    signal: &Signal,
    decoders: &[Measured],
) -> Result<Group> {
    let power = clean.active_power();
    let runs = signal.noise.runs();
    let mut group = Group::new(
        &format!("{}-noise", signal.id),
        &format!("{} in noise", signal.name),
        "% decoded",
        Better::Higher,
    )
    .with_note(&signal.noise.note());
    for measured in decoders {
        let noisy = |index: usize| -> Result<Vec<String>> {
            let fixture = prepared(&clean.with_noise(power, runs[index]), signal)?;
            match &measured.decoder {
                Decoder::Ours => Ok(ours::decode(&fixture, signal, 1)?.keys),
                Decoder::Program(reference, binary) => {
                    once(work, &fixture, front, signal, reference, binary, index)
                }
            }
        };
        let found = match measured.decoder {
            Decoder::Ours => (0..runs.len()).map(noisy).collect::<Result<Vec<_>>>()?,
            Decoder::Program(..) => in_parallel(runs.len(), noisy)?,
        };
        let tallies: Vec<Tally> = found
            .iter()
            .map(|keys| Tally::of(&measured.clean, keys, signal.unique))
            .collect();
        let total = tallies.iter().fold(Tally::default(), |sum, &t| sum.add(t));
        println!(
            "{:<28} {:<12} {:>5.1} %  by level {}",
            group.title,
            measured.name,
            total.percent(),
            by_level(&tallies)
        );
        group.push(measured.name, &measured.version, total.percent());
    }
    Ok(group)
}

fn by_level(tallies: &[Tally]) -> String {
    tallies
        .chunks(noise::COPIES)
        .map(|level| {
            let sum = level.iter().fold(Tally::default(), |sum, &t| sum.add(t));
            format!("{:.0}", sum.percent())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn once(
    work: &Path,
    fixture: &Fixture,
    front: &FrontEnd,
    signal: &Signal,
    reference: &Reference,
    binary: &Path,
    index: usize,
) -> Result<Vec<String>> {
    let path = work.join(format!(
        "{}-{}-noise{index}.{}",
        signal.id,
        slug(reference.program.name),
        reference.format.extension()
    ));
    let tail = if reference.tail {
        signal.tail_seconds
    } else {
        0.0
    };
    convert::write(fixture, front, reference.format, 1, tail, &path)?;
    let keys = invoke(binary, reference, &path)?.keys;
    std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    Ok(keys)
}

fn in_parallel<T: Send>(count: usize, job: impl Fn(usize) -> Result<T> + Sync) -> Result<Vec<T>> {
    let workers = thread::available_parallelism()
        .map_or(4, NonZero::get)
        .min(count.max(1));
    let next = AtomicUsize::new(0);
    let done = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| -> Result<Vec<(usize, T)>> {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= count {
                            return Ok(done);
                        }
                        done.push((index, job(index)?));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| anyhow!("a decoder run panicked"))?
            })
            .collect::<Result<Vec<_>>>()
    })?;
    let mut done: Vec<(usize, T)> = done.into_iter().flatten().collect();
    done.sort_by_key(|(index, _)| *index);
    Ok(done.into_iter().map(|(_, value)| value).collect())
}

fn slug(name: &str) -> String {
    name.replace(' ', "-").to_ascii_lowercase()
}

fn shown(group: Group, show: Show) -> Option<Group> {
    match show {
        Show::Hidden => None,
        Show::Plain => Some(group),
        Show::Noted(note) => Some(group.with_note(note)),
    }
}

fn prepared(fixture: &Fixture, signal: &Signal) -> Result<Fixture> {
    match signal.cu8_rate {
        Some(rate) => fixture.as_cu8(rate),
        None => Ok(fixture.clone()),
    }
}

fn record(
    decodes: &mut Group,
    speed: &mut Group,
    tool: &str,
    version: &str,
    found: usize,
    realtime: f64,
) {
    println!(
        "{:<28} {tool:<12} {found:>4} msgs {realtime:>9.1}× realtime",
        decodes.title
    );
    decodes.push(tool, version, found as f64);
    speed.push(tool, version, realtime);
}

struct Inputs {
    once: PathBuf,
    looped: PathBuf,
}

impl Inputs {
    fn write(
        work: &Path,
        fixture: &Fixture,
        front: &FrontEnd,
        signal: &Signal,
        reference: &Reference,
        loops: usize,
    ) -> Result<Self> {
        let stem = format!("{}-{}", signal.id, slug(reference.program.name));
        let extension = reference.format.extension();
        let once = work.join(format!("{stem}-once.{extension}"));
        let looped = work.join(format!("{stem}-x{loops}.{extension}"));
        let tail = if reference.tail {
            signal.tail_seconds
        } else {
            0.0
        };
        convert::write(fixture, front, reference.format, 1, tail, &once)?;
        convert::write(fixture, front, reference.format, loops, tail, &looped)?;
        Ok(Self { once, looped })
    }
}

struct Output {
    keys: Vec<String>,
    cpu_seconds: f64,
}

fn invoke(binary: &Path, reference: &Reference, input: &Path) -> Result<Output> {
    let input = input.to_string_lossy();
    let args: Vec<&str> = reference
        .args
        .iter()
        .map(|&arg| if arg == INPUT { input.as_ref() } else { arg })
        .collect();
    let dir = binary.parent().context("binary without a directory")?;
    let before = usage::children()?;
    let out = Command::new(binary)
        .args(&args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("run {}", binary.display()))?;
    let cpu_seconds = usage::children()? - before;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let code = out.status.code().unwrap_or(-1);
    ensure!(
        reference.exit_codes.contains(&code),
        "{} exited with {code}: {}",
        reference.program.name,
        stderr.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
    );
    let text = format!("{}\n{stderr}", String::from_utf8_lossy(&out.stdout));
    Ok(Output {
        keys: (reference.keys)(&text),
        cpu_seconds,
    })
}

fn count(signal: &Signal, keys: &[String]) -> usize {
    if signal.unique {
        keys.iter().collect::<BTreeSet<_>>().len()
    } else {
        keys.len()
    }
}

fn median(runs: impl Iterator<Item = Result<f64>>) -> Result<f64> {
    let mut values = runs.collect::<Result<Vec<_>>>()?;
    ensure!(!values.is_empty(), "nothing was measured");
    values.sort_by(f64::total_cmp);
    let value = values[values.len() / 2];
    ensure!(value > 0.0, "a run used no measurable CPU time");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_signals_count_each_message_once() {
        let keys: Vec<String> = ["a", "b", "a"].map(String::from).to_vec();
        let adsb = SIGNALS.iter().find(|s| s.id == "adsb").expect("adsb");
        let dmr = SIGNALS.iter().find(|s| s.id == "dmr").expect("dmr");
        assert_eq!(count(adsb, &keys), 2);
        assert_eq!(count(dmr, &keys), 3);
    }

    #[test]
    fn hidden_groups_are_dropped_and_notes_kept() {
        let group = || Group::new("x", "X", "msgs", Better::Higher);
        assert!(shown(group(), Show::Hidden).is_none());
        assert_eq!(shown(group(), Show::Plain).and_then(|g| g.note), None);
        assert_eq!(
            shown(group(), Show::Noted("n"))
                .and_then(|g| g.note)
                .as_deref(),
            Some("n")
        );
    }

    #[test]
    fn notes_stay_short() {
        for signal in SIGNALS {
            for show in [signal.decodes, signal.speed] {
                if let Show::Noted(note) = show {
                    assert!(note.split_whitespace().count() <= 10, "{note}");
                    assert!(!note.contains('\u{2014}'), "{note}");
                }
            }
        }
    }

    #[test]
    fn one_signal_can_be_chosen() {
        assert_eq!(chosen(None).expect("all").len(), SIGNALS.len());
        assert_eq!(chosen(Some("flex")).expect("flex")[0].id, "flex");
        assert!(chosen(Some("nope")).is_err());
    }

    #[test]
    fn the_median_run_is_reported() {
        let runs = [3.0, 1.0, 2.0].map(Ok);
        assert_eq!(median(runs.into_iter()).expect("median"), 2.0);
        assert!(median([0.0].map(Ok).into_iter()).is_err());
    }
}
