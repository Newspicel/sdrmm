use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn run(program: &str, args: &[&str], cwd: &Path) -> Result<()> {
    run_with_env(program, args, cwd, &[])
}

pub fn run_with_env(program: &str, args: &[&str], cwd: &Path, env: &[(&str, &str)]) -> Result<()> {
    println!("$ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .current_dir(cwd)
        .status()
        .with_context(|| format!("failed to spawn `{program}` (is it installed?)"))?;
    if !status.success() {
        bail!("`{program} {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

pub fn retry(attempts: usize, mut run: impl FnMut() -> Result<()>) -> Result<()> {
    let mut outcome = run();
    for attempt in 2..=attempts {
        if outcome.is_ok() {
            break;
        }
        println!("retrying after ffmpeg-sys build script crash ({attempt}/{attempts})");
        outcome = run();
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_until_success() {
        let mut calls = 0;
        let outcome = retry(3, || {
            calls += 1;
            if calls < 3 { bail!("crash") } else { Ok(()) }
        });
        assert!(outcome.is_ok());
        assert_eq!(calls, 3);
    }

    #[test]
    fn gives_up_after_attempts() {
        let mut calls = 0;
        let outcome = retry(2, || {
            calls += 1;
            bail!("crash")
        });
        assert!(outcome.is_err());
        assert_eq!(calls, 2);
    }
}
