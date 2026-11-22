use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};

use super::parse;

pub struct Program {
    pub name: &'static str,
    pub origin: Origin,
}

pub enum Origin {
    Installed(Installed),
    Source(Source),
}

pub struct Installed {
    pub binary: &'static str,
    pub formula: &'static str,
    pub probe_binary: &'static str,
    pub probe: &'static [&'static str],
    pub marker: &'static str,
}

pub struct Source {
    pub url: &'static str,
    pub commit: &'static str,
    pub version: &'static str,
    pub binary: &'static str,
    pub build: Build,
}

pub enum Build {
    Cmake(&'static [&'static str]),
    Make(&'static [&'static str]),
    Meson,
}

pub const DUMP1090: Program = Program {
    name: "dump1090-fa",
    origin: Origin::Installed(Installed {
        binary: "dump1090",
        formula: "dump1090-fa",
        probe_binary: "dump1090",
        probe: &["--version"],
        marker: "dump1090-fa ",
    }),
};

pub const READSB: Program = Program {
    name: "readsb",
    origin: Origin::Installed(Installed {
        binary: "readsb",
        formula: "readsb",
        probe_binary: "readsb",
        probe: &["--version"],
        marker: "version:",
    }),
};

pub const MINIMODEM: Program = Program {
    name: "minimodem",
    origin: Origin::Installed(Installed {
        binary: "minimodem",
        formula: "minimodem",
        probe_binary: "minimodem",
        probe: &["--version"],
        marker: "minimodem ",
    }),
};

pub const RTL_433: Program = Program {
    name: "rtl_433",
    origin: Origin::Installed(Installed {
        binary: "rtl_433",
        formula: "rtl_433",
        probe_binary: "rtl_433",
        probe: &["-V"],
        marker: "rtl_433 version ",
    }),
};

pub const DIREWOLF: Program = Program {
    name: "Dire Wolf",
    origin: Origin::Installed(Installed {
        binary: "atest",
        formula: "direwolf",
        probe_binary: "direwolf",
        probe: &["-V"],
        marker: "Release",
    }),
};

pub const MULTIMON: Program = Program {
    name: "multimon-ng",
    origin: Origin::Source(Source {
        url: "https://github.com/EliasOenal/multimon-ng.git",
        commit: "0722194b7739748e49f18ac1fc76f236d4ca390d",
        version: "1.6.2",
        binary: "build/multimon-ng",
        build: Build::Cmake(&[]),
    }),
};

pub const AIS_CATCHER: Program = Program {
    name: "AIS-catcher",
    origin: Origin::Source(Source {
        url: "https://github.com/jvde-github/AIS-catcher.git",
        commit: "b6b4ae25c566bd74f2ede9c385b5e899f364ced2",
        version: "0.70",
        binary: "build/AIS-catcher",
        build: Build::Cmake(&[]),
    }),
};

pub const ACARSDEC: Program = Program {
    name: "acarsdec",
    origin: Origin::Source(Source {
        url: "https://github.com/TLeconte/acarsdec.git",
        commit: "339f63eb91a890cfe5b199ad70814cfe86702d1e",
        version: "3.7",
        binary: "build/acarsdec",
        build: Build::Cmake(&[
            "-DCMAKE_POLICY_VERSION_MINIMUM=3.5",
            "-DCMAKE_PREFIX_PATH={brew}",
            "-DCMAKE_C_FLAGS=-DHOST_NAME_MAX=255 -I{brew}/include",
        ]),
    }),
};

pub const DUMP978: Program = Program {
    name: "dump978-fa",
    origin: Origin::Source(Source {
        url: "https://github.com/flightaware/dump978.git",
        commit: "f40aa9a0d2d0a0067c0c628b44ba316f7049b8ce",
        version: "11.1",
        binary: "dump978-fa",
        build: Build::Make(&[
            "CPPFLAGS=-I{brew}/include",
            "LDFLAGS=-L{brew}/lib",
            "dump978-fa",
        ]),
    }),
};

pub const REDSEA: Program = Program {
    name: "redsea",
    origin: Origin::Source(Source {
        url: "https://github.com/windytan/redsea.git",
        commit: "4cc27df9939798e800c4ff7cb484be6d9bf68b4d",
        version: "1.3.1",
        binary: "build/redsea",
        build: Build::Meson,
    }),
};

pub const FT8_LIB: Program = Program {
    name: "ft8_lib",
    origin: Origin::Source(Source {
        url: "https://github.com/kgoba/ft8_lib.git",
        commit: "9fec6ca39886edbf96f4f5e71edc76da5074e871",
        version: "9fec6ca",
        binary: "decode_ft8",
        build: Build::Make(&[
            "CFLAGS=-O3 -DHAVE_STPCPY -I. -include stdio.h",
            "decode_ft8",
        ]),
    }),
};

pub const DSD_FME: Program = Program {
    name: "dsd-fme",
    origin: Origin::Source(Source {
        url: "https://github.com/lwvmobile/dsd-fme.git",
        commit: "4fe32db7f5affb0484aa0e4f8bbc50eabe3c8531",
        version: "4fe32db",
        binary: "build/dsd-fme",
        build: Build::Cmake(&[
            "-DCMAKE_POLICY_VERSION_MINIMUM=3.5",
            "-DCMAKE_PREFIX_PATH={brew}/opt/ncurses;{brew}",
        ]),
    }),
};

pub struct Ready {
    pub binary: PathBuf,
    pub version: String,
}

pub fn prepare(root: &Path, program: &Program) -> Result<Ready> {
    match &program.origin {
        Origin::Installed(installed) => {
            let binary = on_path(installed.binary).with_context(|| {
                format!(
                    "{} not found; install it with `brew install {}`",
                    installed.binary, installed.formula
                )
            })?;
            let version = probe(installed)?;
            Ok(Ready { binary, version })
        }
        Origin::Source(source) => Ok(Ready {
            binary: built(root, program.name, source)?,
            version: source.version.to_owned(),
        }),
    }
}

fn on_path(binary: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(binary))
        .find(|path| path.is_file())
}

fn probe(installed: &Installed) -> Result<String> {
    let binary = on_path(installed.probe_binary)
        .with_context(|| format!("{} not found", installed.probe_binary))?;
    let out = Command::new(&binary)
        .args(installed.probe)
        .output()
        .with_context(|| format!("run {}", binary.display()))?;
    let text = parse::strip_ansi(&format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ));
    parse::word_after(&text, installed.marker)
        .with_context(|| format!("no version in the output of {}", binary.display()))
}

fn built(root: &Path, name: &str, source: &Source) -> Result<PathBuf> {
    let dir = root.join("target/compare/src").join(name);
    let binary = dir.join(source.binary);
    if binary.is_file() && head(&dir)? == source.commit {
        return Ok(binary);
    }
    checkout(&dir, source)?;
    build(&dir, &source.build)?;
    ensure!(
        binary.is_file(),
        "{name} built without {}",
        binary.display()
    );
    Ok(binary)
}

fn head(dir: &Path) -> Result<String> {
    if !dir.join(".git").exists() {
        return Ok(String::new());
    }
    git(dir, &["rev-parse", "HEAD"])
}

fn checkout(dir: &Path, source: &Source) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    if !dir.join(".git").exists() {
        git(dir, &["init", "--quiet"])?;
    }
    git(
        dir,
        &[
            "fetch",
            "--quiet",
            "--depth",
            "1",
            source.url,
            source.commit,
        ],
    )?;
    git(dir, &["checkout", "--quiet", "--force", "FETCH_HEAD"])?;
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("run git")?;
    ensure!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

fn build(dir: &Path, build: &Build) -> Result<()> {
    let brew = brew_prefix();
    let expand = |args: &[&str]| -> Vec<String> {
        args.iter()
            .map(|arg| arg.replace("{brew}", &brew))
            .collect()
    };
    match build {
        Build::Cmake(extra) => {
            let mut configure = vec![
                "-S".to_owned(),
                ".".to_owned(),
                "-B".to_owned(),
                "build".to_owned(),
                "-DCMAKE_BUILD_TYPE=Release".to_owned(),
            ];
            configure.extend(expand(extra));
            step(dir, "cmake", &configure)?;
            step(
                dir,
                "cmake",
                &["--build".into(), "build".into(), "--parallel".into()],
            )
        }
        Build::Make(args) => step(dir, "make", &expand(args)),
        Build::Meson => {
            let setup = ["setup", "build", "--buildtype=release", "--reconfigure"];
            step(dir, "meson", &setup.map(String::from))?;
            step(
                dir,
                "meson",
                &["compile".into(), "-C".into(), "build".into()],
            )
        }
    }
}

fn brew_prefix() -> String {
    Command::new("brew")
        .arg("--prefix")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map_or_else(|| "/usr".to_owned(), |prefix| prefix.trim().to_owned())
}

fn step(dir: &Path, program: &str, args: &[String]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("run {program}"))?;
    if !status.success() {
        bail!("{program} {args:?} failed in {}", dir.display());
    }
    Ok(())
}
