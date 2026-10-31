mod ber;
mod broadcast_fixtures;
mod capture;
mod compare;
mod denoise_model;
mod excerpt;
mod frame_fixtures;
mod fuzz_seeds;
mod icons;
mod ident_matrix;
mod replay;
mod resolved_fixtures;
mod wires;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use num_complex::Complex;
pub(crate) use xtask_release::proc::run;
pub(crate) use xtask_release::web::web_build;

/// The tasks that build the decoders, the server or a model runtime. `cargo xtask` forwards
/// them here so the gates that only drive cargo and pnpm never compile any of it.
#[derive(Parser)]
#[command(name = "xtask-gen", about = "SDRmm generators and analysis tools")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Codegen,
    Fixtures {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    FuzzSeeds,
    BroadcastFixtures {
        #[arg(long)]
        out: PathBuf,
    },
    Excerpt(excerpt::Excerpt),
    Replay(replay::Replay),
    NetCapture(capture::NetCapture),
    Ber {
        entry: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        full: bool,
    },
    DenoiseModel,
    Icons,
    IdentMatrix,
    Compare {
        #[command(subcommand)]
        suite: compare::Compare,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Codegen => codegen(&root()),
        Cmd::Fixtures { out } => fixtures(&out.unwrap_or_else(|| root().join("fixtures"))),
        Cmd::FuzzSeeds => fuzz_seeds::run(&root()),
        Cmd::BroadcastFixtures { out } => broadcast_fixtures::run(&out),
        Cmd::Excerpt(args) => excerpt::run(&root(), &args),
        Cmd::Replay(args) => replay::run(&args),
        Cmd::NetCapture(args) => capture::run(&args),
        Cmd::Ber { entry, out, full } => ber::run(&root(), &entry, out.as_deref(), full),
        Cmd::DenoiseModel => denoise_model::run(&root()),
        Cmd::Icons => icons::icons(&root()),
        Cmd::IdentMatrix => ident_matrix::run(&root()),
        Cmd::Compare { suite } => compare::run(&root(), &suite),
    }
}

#[expect(clippy::expect_used)]
fn root() -> PathBuf {
    xtask_release::root().expect("workspace root")
}

const OPENAPI: &str = "openapi.json";

/// Every file the Rust side generates, paired with its contents. `cargo xtask codegen` writes
/// them and a test holds the committed copies to them, so drift fails the test run rather than
/// a gate that would have to build the server just to look.
fn generated(root: &Path) -> Result<Vec<(&'static str, String)>> {
    let catalog = sdrmm_wire::PatchCatalog::build();
    let spec = sdrmm_server::openapi()
        .to_pretty_json()
        .context("serialize OpenAPI")?;
    Ok(vec![
        (
            "web/src/generated/frame-fixtures.json",
            serde_json::to_string_pretty(&frame_fixtures::frames()?)?,
        ),
        (
            "web/src/generated/catalog-fixture.json",
            serde_json::to_string_pretty(&catalog)?,
        ),
        (
            "web/src/generated/channel-defaults.json",
            serde_json::to_string_pretty(&channel_defaults())?,
        ),
        (
            "web/src/generated/resolved-fixtures.json",
            resolved_fixtures::contents(root)?,
        ),
        (
            "web/src/generated/labels.json",
            serde_json::to_string_pretty(&sdrmm_wire::labels::generated()?)?,
        ),
        (
            "web/src/generated/limits.json",
            sdrmm_wire::limits::generated()?,
        ),
        (
            "web/src/generated/backends.json",
            serde_json::to_string_pretty(&sdrmm_wire::backend_table())?,
        ),
        ("docs/src/wires.md", wires::page(&catalog)),
        (
            "web/src/generated/frame.ts",
            sdrmm_wire::typescript_frames(),
        ),
        (OPENAPI, format!("{spec}\n")),
    ])
}

fn codegen(root: &Path) -> Result<()> {
    for (rel, contents) in generated(root)? {
        let path = root.join(rel);
        std::fs::write(&path, contents).with_context(|| format!("write {rel}"))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

fn channel_defaults() -> std::collections::BTreeMap<String, sdrmm_wire::ChannelParams> {
    sdrmm_channels::descriptors()
        .into_iter()
        .filter_map(|descriptor| {
            descriptor
                .defaults
                .map(|settings| (descriptor.type_id, settings.params))
        })
        .collect()
}

fn fixtures(dir: &Path) -> Result<()> {
    const CENTER_HZ: f64 = 100_000_000.0;

    std::fs::create_dir_all(dir).context("create fixtures dir")?;

    const BAND_RATE: f64 = 2_400_000.0;
    let samples = sdrmm_device_virtual::render(BAND_RATE, BAND_RATE as usize);
    write_fixture(
        dir,
        "siggen_2m4_1s",
        &samples,
        BAND_RATE,
        CENTER_HZ,
        "Signal Generator (virtual)",
        "1 s of the virtual test band: the record/replay fixture",
    )?;

    for recipe in sdrmm_channels::synth::decoder_fixtures::RECIPES {
        write_fixture(
            dir,
            recipe.stem,
            &(recipe.iq)(),
            recipe.rate,
            CENTER_HZ,
            "SDRmm reference modulator",
            recipe.note,
        )?;
    }
    Ok(())
}

fn write_fixture(
    dir: &Path,
    stem_name: &str,
    iq: &[Complex<f32>],
    rate: f64,
    center_hz: f64,
    hw: &str,
    note: &str,
) -> Result<()> {
    let stem = dir.join(stem_name);
    for path in [
        sdrmm_recorder::meta_path(&stem),
        sdrmm_recorder::data_path(&stem),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err).with_context(|| format!("replace {}", path.display())),
        }
    }
    let mut writer = sdrmm_recorder::SigmfWriter::create(&stem, rate, center_hz, hw)
        .with_context(|| format!("create fixture {stem_name}"))?;
    writer
        .write_block(iq)
        .with_context(|| format!("write fixture {stem_name}"))?;
    writer
        .finalize()
        .with_context(|| format!("finalize fixture {stem_name}"))?;

    let reader = sdrmm_recorder::SigmfReader::open(&stem)
        .with_context(|| format!("re-open fixture {stem_name}"))?;
    ensure!(
        reader.total_samples() == iq.len() as u64,
        "fixture {stem_name} readback: {} samples on disk, {} rendered",
        reader.total_samples(),
        iq.len()
    );
    println!(
        "{stem_name}: {} samples, {:.2} s @ {}: {note}",
        iq.len(),
        iq.len() as f64 / rate,
        sdrmm_wire::units::sample_rate(rate),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_files_match_the_committed_copies() {
        let root = root();
        let stale: Vec<_> = generated(&root)
            .expect("generate")
            .into_iter()
            .filter(|(rel, contents)| {
                std::fs::read_to_string(root.join(rel)).ok().as_ref() != Some(contents)
            })
            .map(|(rel, _)| rel)
            .collect();
        assert!(
            stale.is_empty(),
            "codegen drift in {stale:?}: regenerate with `cargo xtask codegen` and commit"
        );
    }
}
