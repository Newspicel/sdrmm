import argparse
import filecmp
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

import dvbs2_spec

MODULATIONS = {2: "Qpsk", 3: "Psk8", 4: "Apsk16", 5: "Apsk32", 6: "Apsk64", 7: "Apsk128", 8: "Apsk256"}
BASE_TABLES = {"B.1", "C.1", "C.2"}
RUSTFMT_CONFIG = Path(__file__).resolve().parents[3] / "rustfmt.toml"


def float32(value):
    target = struct.pack("<f", value)
    value = dvbs2_spec.single(value)
    text = next(
        f"{value:.{digits}g}" for digits in range(1, 10) if struct.pack("<f", float(f"{value:.{digits}g}")) == target
    )
    return text if "." in text or "e" in text else text + ".0"


def load(cache):
    s2 = dvbs2_spec.Standard(cache, "s2")
    s2x = dvbs2_spec.Standard(cache, "s2x")
    tables = {key: dvbs2_spec.ldpc_rows(table) for key, table in dvbs2_spec.ldpc_tables(s2).items()}
    extension = dvbs2_spec.ldpc_tables(s2x)
    for key, table in extension.items():
        if key in tables:
            raise ValueError(f"LDPC table {key} defined twice")
        tables[key] = dvbs2_spec.ldpc_rows(table)
    base = set(tables) - {key for key, table in extension.items() if table.table not in BASE_TABLES}
    return dvbs2_spec.s2x_modes(s2, s2x), tables, base


def write_points(root, modes):
    folder = root / "points"
    folder.mkdir(parents=True, exist_ok=True)
    lines = ["use super::{Mode, Modulation, Rate};", ""]
    for mode in modes:
        name = f"m{mode['code']}"
        lines.append(f"mod {name};")
        body = "".join(f"    ({float32(a)}, {float32(b)}),\n" for a, b in mode["points"])
        (folder / f"{name}.rs").write_text("pub const POINTS: &[(f32, f32)] = &[\n" + body + "];\n")
    lines += ["", "pub const MODES: &[Mode] = &["]
    for mode in modes:
        modulation = "Apsk8" if mode["family"] == "2+4+2APSK" else MODULATIONS[mode["bits"]]
        lines.append(
            f"    Mode {{ code: {mode['code']}, short: {str(mode['short']).lower()}, "
            f"modulation: Modulation::{modulation}, rate: Rate::R{mode['rate']}, "
            f"order: &{mode['order']}, points: m{mode['code']}::POINTS }},"
        )
    lines.append("];")
    (folder / "mod.rs").write_text("\n".join(lines) + "\n")


def write_tables(root, modes, tables, base):
    folder = root / "s2x"
    folder.mkdir(parents=True, exist_ok=True)
    needed = sorted({(mode["rate"], "S" if mode["short"] else "N") for mode in modes} - base)
    lines = ["use crate::dvb::ldpc::{Frame, Rate};", ""]
    arms = []
    for rate, size in needed:
        name = f"r{rate}_{size.lower()}"
        lines.append(f"mod {name};")
        rows = "".join("    &[" + ", ".join(map(str, row)) + "],\n" for row in tables[(rate, size)])
        (folder / f"{name}.rs").write_text("pub const ADDRESSES: &[&[u16]] = &[\n" + rows + "];\n")
        frame = "Short" if size == "S" else "Normal"
        arms.append(f"        (Rate::R{rate}, Frame::{frame}) => Some({name}::ADDRESSES),")
    lines += [
        "",
        "pub fn addresses(rate: Rate, frame: Frame) -> Option<&'static [&'static [u16]]> {",
        "    match (rate, frame) {",
        *arms,
        "        _ => None,",
        "    }",
        "}",
    ]
    (root / "s2x.rs").write_text("\n".join(lines) + "\n")


def owned(points, tables):
    """The files the generator owns under the points and LDPC table folders."""
    files = [*(points / "points").rglob("*.rs"), *(tables / "s2x").rglob("*.rs")]
    if (tables / "s2x.rs").exists():
        files.append(tables / "s2x.rs")
    return sorted(files)


def generate(points, tables_root, modes, tables, base):
    write_points(points, modes)
    write_tables(tables_root, modes, tables, base)
    files = owned(points, tables_root)
    subprocess.run(["rustfmt", "--edition", "2024", "--config-path", str(RUSTFMT_CONFIG), *map(str, files)], check=True)


def relative(path, roots):
    return next((index, path.relative_to(root)) for index, root in enumerate(roots) if path.is_relative_to(root))


def differences(generated, committed):
    made = {relative(path, generated) for path in owned(*generated)}
    kept = {relative(path, committed) for path in owned(*committed)}
    changed = [
        committed[index] / path
        for index, path in sorted(made)
        if (index, path) not in kept
        or not filecmp.cmp(generated[index] / path, committed[index] / path, shallow=False)
    ]
    stale = [committed[index] / path for index, path in sorted(kept - made)]
    return changed + stale


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--points", type=Path, required=True, help="folder holding the S2X points module")
    parser.add_argument("--tables", type=Path, required=True, help="folder holding the LDPC tables")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    modes, tables, base = load(args.cache)
    if not args.check:
        generate(args.points, args.tables, modes, tables, base)
        return
    with tempfile.TemporaryDirectory(prefix="s2x-tables-") as temporary:
        generated = (Path(temporary) / "points", Path(temporary) / "tables")
        generate(*generated, modes, tables, base)
        mismatched = differences(generated, (args.points, args.tables))
    for path in mismatched:
        print(f"differs from ETSI EN 302 307-2: {path}", file=sys.stderr)
    sys.exit(1 if mismatched else 0)


if __name__ == "__main__":
    main()
