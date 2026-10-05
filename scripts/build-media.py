import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import urllib.request


VERSION = "9.0.2"
SHA256 = "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
FDK_VERSION = "2.0.3"
FDK_SHA256 = "e25671cd96b10bad896aa42ab91a695a9e573395262baed4e4a2ff178d6a3a78"
ROOT = Path(__file__).resolve().parents[1]


def target_name():
    machine = platform.machine().lower()
    arch = "aarch64" if machine in ("arm64", "aarch64") else "x86_64"
    suffix = {"Darwin": "apple-darwin", "Linux": "unknown-linux-gnu", "Windows": "pc-windows-msvc"}[platform.system()]
    return f"{arch}-{suffix}"


# Both stay out of `target/`: Swatinem/rust-cache prunes that directory on a partial cache restore
# and would delete the installed libraries between the build and the link that needs them.
def install_prefix(target):
    return ROOT / ".media" / target


def work_dir(target):
    return ROOT / ".media-build" / target


def shell_env():
    env = os.environ.copy()
    extra = env.get("MEDIA_SHELL_BIN")
    if extra:
        env["PATH"] = os.pathsep.join([extra, env.get("PATH", "")])
    return env


# Windows `CreateProcess` resolves a bare name against the parent's `PATH`, not the one handed to
# the child, so every tool is looked up in the shell environment and spawned by absolute path.
def tool(name, env):
    found = shutil.which(name, path=env.get("PATH"))
    if found is None:
        raise RuntimeError(f"{name} is required to build FFmpeg")
    return found


def run(args, cwd, env):
    subprocess.run(args, cwd=cwd, env=env, check=True)


def require_msys_shell(env):
    kernel = subprocess.run(
        [tool("bash", env), "-c", "uname -s"], env=env, capture_output=True, text=True, check=True
    ).stdout
    if kernel.startswith("CYGWIN"):
        raise RuntimeError("Cygwin bash cannot build FFmpeg for clang-cl, put MSYS2 usr\\bin in MEDIA_SHELL_BIN")


def unpack(work, archive, url, sha256, name):
    if not archive.exists():
        with urllib.request.urlopen(url) as response:
            archive.write_bytes(response.read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != sha256:
        raise RuntimeError(f"{name} source checksum mismatch")
    source = work / name
    if not source.exists():
        with tarfile.open(archive) as source_archive:
            source_archive.extractall(work, filter="data")
    return source


def prepare_source(work, archive):
    return unpack(
        work, archive or work / f"ffmpeg-{VERSION}.tar.xz",
        f"https://ffmpeg.org/releases/ffmpeg-{VERSION}.tar.xz", SHA256, f"ffmpeg-{VERSION}",
    )


def prepare_fdk(work):
    return unpack(
        work, work / f"fdk-aac-{FDK_VERSION}.tar.gz",
        f"https://github.com/mstorsjo/fdk-aac/archive/refs/tags/v{FDK_VERSION}.tar.gz", FDK_SHA256,
        f"fdk-aac-{FDK_VERSION}",
    )


def fdk_toolchain(target):
    arch = target.split("-", 1)[0]
    if "windows-msvc" in target:
        return [
            "-G", "NMake Makefiles", "-DCMAKE_C_COMPILER=clang-cl", "-DCMAKE_CXX_COMPILER=clang-cl",
            "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
        ]
    if "apple-darwin" in target:
        return [f"-DCMAKE_OSX_ARCHITECTURES={'arm64' if arch == 'aarch64' else arch}"]
    if target != target_name():
        return [
            "-DCMAKE_SYSTEM_NAME=Linux", f"-DCMAKE_SYSTEM_PROCESSOR={arch}",
            f"-DCMAKE_C_COMPILER={arch}-linux-gnu-gcc", f"-DCMAKE_CXX_COMPILER={arch}-linux-gnu-g++",
        ]
    return []


def build_fdk(source, prefix, target, env):
    build = source.parent / "fdk-build"
    cmake = tool("cmake", env)
    run([
        cmake, "-S", str(source), "-B", str(build), "-DCMAKE_BUILD_TYPE=Release",
        "-DBUILD_SHARED_LIBS=OFF", "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
        "-DFDK_AAC_INSTALL_CMAKE_CONFIG_MODULE=OFF", "-DCMAKE_INSTALL_LIBDIR=lib",
        f"-DCMAKE_INSTALL_PREFIX={prefix.as_posix()}",
        *fdk_toolchain(target),
    ], source.parent, env)
    run([cmake, "--build", str(build), "--parallel", str(os.cpu_count() or 2)], source.parent, env)
    run([cmake, "--install", str(build)], source.parent, env)


def configure(source, prefix, fdk, target, env):
    args = [
        tool("bash", env), str(source / "configure"), f"--prefix={prefix.as_posix()}",
        "--disable-autodetect", "--disable-everything", "--disable-network",
        "--disable-programs", "--disable-doc", "--disable-debug", "--enable-shared",
        "--disable-static", "--enable-pic", "--disable-avdevice", "--disable-avfilter",
        "--enable-avcodec", "--enable-avformat", "--enable-swresample", "--enable-swscale",
        "--enable-decoder=aac,aac_latm,ac3,eac3,mp2,mpeg2video,h264,hevc,libfdk_aac",
        "--enable-parser=aac,aac_latm,ac3,mpegaudio,mpegvideo,h264,hevc",
        "--enable-libfdk-aac", f"--extra-cflags=-I{(fdk / 'include').as_posix()}",
        f"--extra-ldflags=-L{(fdk / 'lib').as_posix()}",
    ]
    arch = target.split("-", 1)[0]
    args.append(f"--arch={arch}")
    if not shutil.which("nasm", path=env.get("PATH")):
        args.append("--disable-x86asm")
    if "windows-msvc" in target:
        # `lib.exe` rather than `llvm-lib`: configure picks an archiver's flags by asking it who it
        # is, and only the Microsoft banner selects `-out:`. llvm-lib answers with nothing configure
        # recognises, so it falls back to `ar rc` syntax and llvm-lib reads `rc` as a missing input.
        args.extend(["--toolchain=msvc", "--target-os=win32", "--cc=clang-cl", "--ld=lld-link"])
        # FFmpeg assembles its aarch64 kernels with armasm64 behind gas-preprocessor.pl, and the
        # Windows ARM64 runner carries neither.
        if arch == "aarch64":
            args.append("--disable-asm")
    elif "apple-darwin" in target:
        args.extend([
            "--target-os=darwin", "--install-name-dir=@rpath", "--extra-ldsoflags=-Wl,-rpath,@loader_path",
            f"--cc=clang -arch {'arm64' if arch == 'aarch64' else arch}",
        ])
        if target != target_name():
            args.append("--enable-cross-compile")
    elif target != target_name():
        args.extend(["--enable-cross-compile", "--target-os=linux", f"--cross-prefix={arch}-linux-gnu-"])
    return args


def move_import_libraries(prefix):
    lib = prefix / "lib"
    lib.mkdir(parents=True, exist_ok=True)
    for library in (prefix / "bin").glob("*.lib"):
        library.replace(lib / library.name)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", default=target_name())
    parser.add_argument("--prefix", type=Path)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--print-prefix", action="store_true")
    args = parser.parse_args()
    prefix = (args.prefix or install_prefix(args.target)).resolve()
    if args.print_prefix:
        print(prefix)
        return
    fingerprint = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    marker = prefix / "sdrmm-build.txt"
    if marker.exists() and marker.read_text() == fingerprint:
        print(prefix)
        return
    work = work_dir(args.target)
    work.mkdir(parents=True, exist_ok=True)
    env = shell_env()
    if "windows-msvc" in args.target:
        require_msys_shell(env)
    fdk = work / "fdk-aac"
    build_fdk(prepare_fdk(work), fdk, args.target, env)
    env["PKG_CONFIG_PATH"] = os.pathsep.join([str(fdk / "lib" / "pkgconfig"), env.get("PKG_CONFIG_PATH", "")])
    source = prepare_source(work, args.archive)
    run(configure(source, prefix, fdk, args.target, env), work, env)
    run([tool("make", env), "-j", str(os.cpu_count() or 2)], work, env)
    shutil.rmtree(prefix, ignore_errors=True)
    run([tool("make", env), "install"], work, env)
    if "windows-msvc" in args.target:
        move_import_libraries(prefix)
    marker.write_text(fingerprint)
    print(prefix)


if __name__ == "__main__":
    main()
