#!/usr/bin/env python3
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROGRAMS_DIR = ROOT / "programs"
BOOTFS_PROGRAMS_DIR = ROOT / "bootfs" / "programs"
BOOTFS_BUILD_DIR = ROOT / "bootfs" / "build"
PACKAGE_MANIFEST = ROOT / "rymos-packages.toml"
SELFHOST_MANIFEST = ROOT / "rymos-selfhost.toml"
CUSTOM_TARGET = ROOT / "targets" / "x86_64-rymos.json"
STABLE_TARGET = "x86_64-unknown-none"
LOAD_ADDR = "0x200000"
STD_TOOLCHAIN = "rymos-fork"
TOOLCHAIN_RUST_DIR = ROOT / "toolchain" / "rust"


def main() -> int:
    if len(sys.argv) < 2 or sys.argv[1] in {"help", "-h", "--help"}:
        usage()
        return 0

    command = sys.argv[1]
    program = sys.argv[2] if len(sys.argv) > 2 else "hello"

    if command == "list":
        for app in discover_apps():
            print(f"{app['dir']}: package={app['package']} bin={app['bin']}")
        return 0
    if command == "pkg-list":
        for package in enabled_packages():
            print(
                f"{package['name']}: package={package['package']} "
                f"bin={package['bin']} install={package['install']}"
            )
        return 0
    if command == "build":
        build_program(program)
        return 0
    if command == "install":
        install_program(program)
        return 0
    if command == "build-all":
        for package in enabled_packages():
            build_program(package["name"])
        return 0
    if command == "install-all":
        installed = []
        for package in enabled_packages():
            installed.append(install_program(package["name"]))
        write_package_index(installed)
        write_selfhost_report()
        return 0
    if command == "selfhost-status":
        write_selfhost_report()
        return 0

    print(f"unknown sdk command: {command}", file=sys.stderr)
    usage()
    return 2


def usage() -> None:
    print("RYMOS SDK")
    print("")
    print("Usage:")
    print("  scripts/rymos-sdk.py list")
    print("  scripts/rymos-sdk.py pkg-list")
    print("  scripts/rymos-sdk.py build [program]")
    print("  scripts/rymos-sdk.py install [program]")
    print("  scripts/rymos-sdk.py build-all")
    print("  scripts/rymos-sdk.py install-all")
    print("  scripts/rymos-sdk.py selfhost-status")
    print("")
    print("Environment:")
    print("  RYMOS_TARGET_MODE=auto|stable|custom|std  default: auto")
    print("    std: real std via the rymos-fork toolchain (see")
    print("    docs/dev-environment.md) -- only for programs that opt into")
    print("    real std (today: stdreal); never picked by auto")


def discover_apps() -> list[dict[str, str]]:
    apps = []
    for manifest in sorted(PROGRAMS_DIR.glob("*/Cargo.toml")):
        data = read_toml(manifest)
        package = data.get("package", {}).get("name")
        bin_entries = data.get("bin", [])
        bin_name = None
        if bin_entries:
            bin_name = bin_entries[0].get("name")
        bin_name = bin_name or package_name_to_bin(package)
        if package and bin_name:
            apps.append(
                {
                    "dir": manifest.parent.name,
                    "package": package,
                    "bin": bin_name,
                }
            )
    return apps


def enabled_packages() -> list[dict[str, str]]:
    if not PACKAGE_MANIFEST.exists():
        raise SystemExit(f"missing package manifest: {PACKAGE_MANIFEST.relative_to(ROOT)}")
    data = read_toml(PACKAGE_MANIFEST)
    packages = []
    for entry in data.get("program", []):
        if not entry.get("enabled", True):
            continue
        package = normalize_package_entry(entry)
        validate_package_entry(package)
        packages.append(package)
    return packages


def normalize_package_entry(entry: dict[str, object]) -> dict[str, str]:
    package = {
        "name": str(entry.get("name", "")).strip(),
        "package": str(entry.get("package", "")).strip(),
        "bin": str(entry.get("bin", "")).strip(),
        "source": str(entry.get("source", "")).strip(),
        "install": str(entry.get("install", "")).strip(),
    }
    if not package["install"] and package["bin"]:
        package["install"] = f"programs/{package['bin']}.elf"
    return package


def validate_package_entry(package: dict[str, str]) -> None:
    required = ["name", "package", "bin", "source", "install"]
    missing = [key for key in required if not package[key]]
    if missing:
        raise SystemExit(f"invalid package entry {package}: missing {', '.join(missing)}")
    app = find_app(package["name"])
    if app["package"] != package["package"]:
        raise SystemExit(
            f"package manifest mismatch for {package['name']}: "
            f"{package['package']} != {app['package']}"
        )
    if app["bin"] != package["bin"]:
        raise SystemExit(
            f"package manifest mismatch for {package['name']}: "
            f"{package['bin']} != {app['bin']}"
        )
    source = ROOT / package["source"]
    if not source.exists():
        raise SystemExit(f"package source missing: {package['source']}")
    install = Path(package["install"])
    if install.is_absolute() or ".." in install.parts:
        raise SystemExit(f"invalid bootfs install path: {package['install']}")


def find_app(name: str) -> dict[str, str]:
    for app in discover_apps():
        if name in {app["dir"], app["package"], app["bin"]}:
            return app
    known = ", ".join(app["dir"] for app in discover_apps()) or "<none>"
    raise SystemExit(f"unknown RYMOS program '{name}' (known: {known})")


def build_program(name: str) -> dict[str, str]:
    app = find_app(name)
    mode = target_mode()
    print(f"Building {app['package']} for RYMOS ({mode} target mode)", flush=True)
    if mode == "std":
        prepare_std_build()
    subprocess.run(build_command(app, mode), cwd=ROOT, env=build_env(mode), check=True)
    return app


def prepare_std_build() -> None:
    # A stale `-Z build-std` cache for this target can silently keep using
    # pre-edit `library/std` sources after a real change -- confirmed live
    # once already (see docs/self-hosting.md, category 5/6): a genuine bug
    # fix looked like it hadn't taken effect at all until this exact
    # directory was removed. Always start clean for std-mode builds rather
    # than risk it -- this build is small/infrequent enough that the
    # rebuild cost doesn't matter.
    stale = ROOT / "target" / CUSTOM_TARGET.stem
    if stale.exists():
        shutil.rmtree(stale)

    # `x.py build library/std` regenerates the stage1 sysroot's
    # lib/rustlib/$HOST/bin/ directory on every rebuild, wiping this symlink
    # each time -- see docs/dev-environment.md's "rust-lld gotcha". Recreate
    # it here so a std-mode build doesn't fail with "linker `rust-lld` not
    # found" just because the toolchain was rebuilt since the last install.
    host = host_triple()
    stage1_bin = TOOLCHAIN_RUST_DIR / "build" / "host" / "stage1" / "lib" / "rustlib" / host / "bin"
    lld_link = stage1_bin / "rust-lld"
    lld_target = (
        TOOLCHAIN_RUST_DIR / "build" / host / "stage0-sysroot" / "lib" / "rustlib" / host / "bin" / "rust-lld"
    )
    if not lld_target.exists():
        raise SystemExit(
            f"missing stage0 rust-lld at {lld_target.relative_to(ROOT)} -- "
            "build the rymos-fork toolchain first (docs/dev-environment.md, step 6)"
        )
    stage1_bin.mkdir(parents=True, exist_ok=True)
    if lld_link.is_symlink() or lld_link.exists():
        lld_link.unlink()
    lld_link.symlink_to(lld_target)


def host_triple() -> str:
    result = subprocess.run(
        ["rustc", "-vV"], text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=True
    )
    for line in result.stdout.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise SystemExit("could not determine host triple from `rustc -vV`")


def strip_std_binary(path: Path) -> None:
    # Real `std` binaries carry a lot more debug info than `no_std` ones
    # even in release builds -- strip it so bootfs/the initrd stay small
    # (see docs/dev-environment.md's note on debug binary size).
    host = host_triple()
    objcopy = (
        TOOLCHAIN_RUST_DIR / "build" / "host" / "stage1" / "lib" / "rustlib" / host / "bin" / "rust-objcopy"
    )
    if not objcopy.exists():
        raise SystemExit(f"missing {objcopy.relative_to(ROOT)} -- build the rymos-fork toolchain first")
    subprocess.run([str(objcopy), "--strip-debug", str(path)], check=True)


def install_program(name: str) -> dict[str, str]:
    app = build_program(name)
    mode = target_mode()
    package = package_for_app(app)
    source = ROOT / "target" / target_dir(mode) / "release" / app["bin"]
    dest = ROOT / "bootfs" / package["install"]
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, dest)
    if mode == "std":
        strip_std_binary(dest)
    print(f"Installed {source.relative_to(ROOT)} -> {dest.relative_to(ROOT)}")
    return {
        "name": package["name"],
        "package": app["package"],
        "bin": app["bin"],
        "source": str(source.relative_to(ROOT)),
        "install": str(dest.relative_to(ROOT)),
    }


def package_for_app(app: dict[str, str]) -> dict[str, str]:
    for package in enabled_packages():
        if app["dir"] == package["name"] or app["package"] == package["package"]:
            return package
    return {
        "name": app["dir"],
        "package": app["package"],
        "bin": app["bin"],
        "source": f"programs/{app['dir']}",
        "install": f"programs/{app['bin']}.elf",
    }


def write_package_index(installed: list[dict[str, str]]) -> None:
    BOOTFS_BUILD_DIR.mkdir(parents=True, exist_ok=True)
    lines = ["# RYMOS installed packages", ""]
    for package in installed:
        lines.append(
            f"{package['name']} {package['package']} {package['bin']} {package['install']}"
        )
    lines.append("")
    index = BOOTFS_BUILD_DIR / "packages.txt"
    index.write_text("\n".join(lines))
    print(f"Wrote {index.relative_to(ROOT)}")


def write_selfhost_report() -> None:
    if not SELFHOST_MANIFEST.exists():
        raise SystemExit(f"missing selfhost manifest: {SELFHOST_MANIFEST.relative_to(ROOT)}")
    data = read_toml(SELFHOST_MANIFEST)
    meta = data.get("selfhost", {})
    requirements = data.get("requirement", [])
    BOOTFS_BUILD_DIR.mkdir(parents=True, exist_ok=True)
    lines = [
        "# RYMOS self-hosting status",
        "",
        f"name: {meta.get('name', 'RYMOS self-hosting')}",
        f"milestone: {meta.get('milestone', 'unknown')}",
        f"goal: {meta.get('goal', '')}",
        "",
    ]
    counts: dict[str, int] = {}
    for requirement in requirements:
        status = str(requirement.get("status", "unknown"))
        counts[status] = counts.get(status, 0) + 1
    if counts:
        summary = ", ".join(f"{status}={count}" for status, count in sorted(counts.items()))
        lines.append(f"summary: {summary}")
        lines.append("")
    for requirement in requirements:
        name = requirement.get("name", "Unnamed")
        status = requirement.get("status", "unknown")
        detail = requirement.get("detail", "")
        lines.append(f"[{status}] {name}")
        lines.append(f"  {detail}")
    lines.append("")
    report = BOOTFS_BUILD_DIR / "selfhost.txt"
    report.write_text("\n".join(lines))
    print(f"Wrote {report.relative_to(ROOT)}")


def target_mode() -> str:
    requested = os.environ.get("RYMOS_TARGET_MODE", "auto")
    if requested not in {"auto", "stable", "custom", "std"}:
        raise SystemExit("RYMOS_TARGET_MODE must be auto, stable, custom, or std")
    if requested == "auto":
        # "std" (real std via the forked rymos-fork toolchain) is never
        # picked automatically -- it needs that toolchain link to exist
        # locally (see docs/dev-environment.md) and only makes sense for a
        # program that actually opts into real std (today just `stdreal`),
        # not as a blanket default for every no_std program.
        return "custom" if cargo_is_nightly() else "stable"
    return requested


def cargo_is_nightly() -> bool:
    result = subprocess.run(
        ["cargo", "--version"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return "nightly" in result.stdout


def build_command(app: dict[str, str], mode: str) -> list[str]:
    if mode == "std":
        # Real `std` only builds through the forked rust-lang/rust checkout
        # (`toolchain/rust`, linked locally as the `rymos-fork` rustup
        # toolchain -- see docs/dev-environment.md), not whatever toolchain
        # `cargo` would otherwise resolve to.
        command = ["rustup", "run", STD_TOOLCHAIN, "cargo"]
        command += [
            "-Z",
            "build-std=core,alloc,std,panic_abort,compiler_builtins",
            "-Z",
            "build-std-features=compiler-builtins-mem",
            "-Z",
            "json-target-spec",
        ]
    else:
        command = ["cargo"]
        if mode == "custom":
            command += [
                "-Z",
                "build-std=core,alloc,compiler_builtins",
                "-Z",
                "build-std-features=compiler-builtins-mem",
                "-Z",
                "json-target-spec",
            ]
    command += ["build", "-p", app["package"], "--release", "--target", target_arg(mode)]
    return command


def build_env(mode: str) -> dict[str, str]:
    env = os.environ.copy()
    if mode == "stable":
        flags = [
            "-C relocation-model=static",
            "-C code-model=kernel",
            f"-C link-arg=--defsym=RYMOS_LOAD_ADDR={LOAD_ADDR}",
            "-C link-arg=-Tkernel/linker.ld",
            "-C link-arg=--no-pie",
        ]
        env["RUSTFLAGS"] = " ".join(flags)
    return env


def target_arg(mode: str) -> str:
    if mode in {"custom", "std"}:
        return str(CUSTOM_TARGET.relative_to(ROOT))
    return STABLE_TARGET


def target_dir(mode: str) -> str:
    if mode in {"custom", "std"}:
        return CUSTOM_TARGET.stem
    return STABLE_TARGET


def read_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def package_name_to_bin(package: str | None) -> str | None:
    if not package:
        return None
    prefix = "rymos-program-"
    return package.removeprefix(prefix)


if __name__ == "__main__":
    raise SystemExit(main())
