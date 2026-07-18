# RYMOS SDK And Packages

Milestone 5 adds the first SDK shape for building RYMOS programs. Milestone 6
adds a package manifest and install-all flow for the base system.

## Target

The canonical program target spec lives at:

```text
targets/x86_64-rymos.json
```

It defines the intended RYMOS program ABI:

- `x86_64`
- `os = "rymos"`
- static relocation model
- kernel code model while programs still run in trusted kernel mode
- panic abort
- red zone disabled
- linked at `0x200000` with `kernel/linker.ld`

Cargo currently requires nightly plus `rust-src` and `-Z build-std -Z
json-target-spec` for JSON target specs. The SDK therefore defaults to a
stable-compatible fallback target, `x86_64-unknown-none`, while passing the
same linker contract. See `docs/dev-environment.md` for the exact setup
steps.

To force the custom JSON target later:

```sh
RYMOS_TARGET_MODE=custom scripts/rymos-sdk.py build hello
```

That path expects a nightly-capable toolchain with `rust-src`.

## Commands

List known programs:

```sh
make sdk-list
```

List enabled packages:

```sh
make pkg-list
```

Build and install all enabled packages:

```sh
make programs
```

Build and install one program:

```sh
make program PROGRAM=hello
```

Direct SDK usage:

```sh
scripts/rymos-sdk.py list
scripts/rymos-sdk.py pkg-list
scripts/rymos-sdk.py build hello
scripts/rymos-sdk.py install hello
scripts/rymos-sdk.py build-all
scripts/rymos-sdk.py install-all
scripts/rymos-sdk.py selfhost-status
```

`install` copies the program ELF into:

```text
bootfs/programs/<name>.elf
```

The next `make image` packages it into `INITRD.RFS`.

`install-all` also writes an installed package index:

```text
bootfs/build/packages.txt
```

It also refreshes the compiler-readiness report:

```text
bootfs/build/selfhost.txt
```

## Package Manifest

The base package manifest is:

```text
rymos-packages.toml
```

Current format:

```toml
[package_set]
name = "base"
version = "0.1.0"
description = "Base RYMOS boot programs"

[[program]]
name = "hello"
package = "rymos-program-hello"
bin = "hello"
source = "programs/hello"
install = "programs/hello.elf"
enabled = true
```

The SDK validates package entries against the local workspace package before
building. For now packages are local only; registry/network installs come later.

## Program Layout

Each program lives under `programs/<name>/` and is a workspace package. The
current convention is:

```toml
[package]
name = "rymos-program-hello"

[dependencies]
rymos-user = { path = "../../runtime/rymos-user" }

[[bin]]
name = "hello"
path = "src/main.rs"
```

The binary name becomes the command name used by the RYMOS shell:

```text
run hello
```

`rysh` follows the same package shape and is installed as:

```text
bootfs/programs/rysh.elf
```
