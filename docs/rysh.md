# RYSH Tiny Language

Milestone 7 adds `rysh`, a tiny interpreted language that runs as a normal
RYMOS program.

`rysh` is intentionally small. It proves that RYMOS can run an interpreter
inside the program ABI before we attempt anything as large as the Rust compiler.

## Run

The default demo script is packaged at:

```text
bootfs/build/demo.rym
```

Inside RYMOS:

```text
run rysh build/demo.rym
```

`autoexec.bat` runs this demo at boot.

## Language

Comments start with `#`.

Current commands:

```text
print <text>       print text with newline
write <text>       print text without newline
pid                print the current process ID
args               print raw program arguments
set <name> <text>  store a variable
get <name>         print a variable
cat <path>         print a BootFS file
writefile <path> <text>
                  write a file through descriptor IO
stat <path>       print file metadata
listdir [pfs:]    list BootFS or persistent files
mkdir <pfs:dir>   create a persistent directory
rm <path>         remove a persistent file or empty directory
rename <old> <new>
                  rename a persistent file or empty directory
fillfile <path> <bytes>
                  write a repeated A-Z pattern
countfile <path>  count bytes by reading to EOF
env               list environment variables
getenv <key>      print one environment variable
spawn <name> [args]
                  request a child process launch
wait <pid>        print process status from the ABI
```

`print` and `write` expand variables:

```text
$pid       current process ID
$args      raw program arguments
$name      variable from set/get
```

Example:

```text
# RYMOS tiny language demo
print RYSH tiny language running inside RYMOS
pid
args
set greeting hello-from-script
print $greeting
cat build/packages.txt
```

## Limits

The first interpreter has no heap allocation and uses fixed buffers:

- 1024-byte script read buffer
- 8 variables
- 16-byte variable names
- 64-byte variable values
- 512-byte streaming read buffer

This keeps it compatible with the current runtime while we still lack a real
userspace allocator and filesystem handles.

RYMFS files can now be up to 256 MiB each. From macOS, upload a file into the
persistent disk image with:

```sh
make pfs-put UPLOAD_FILE=/path/to/file.bin UPLOAD_DEST=uploads/file.bin
```

Persistent files use the `pfs:` prefix:

```text
mkdir pfs:src
writefile pfs:src/main.rs fn-main
rename pfs:src/main.rs pfs:src/lib.rs
rm pfs:src/lib.rs
fillfile pfs:src/big.bin 6000
countfile pfs:src/big.bin
listdir pfs:src
writefile pfs:fd-demo.txt hello-from-pfs-fd
cat pfs:fd-demo.txt
```
