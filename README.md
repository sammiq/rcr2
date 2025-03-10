rcr2: a rom auditing tool in Rust
=======================================

This tool uses logiqx xml format dat files, as provided by your friendly preservation site, for verifying your own dumps
against known good versions of the same software.

This is a complete rewrite of `rcr`, and is very much a work in progress.

History
-------

While looking around for a simple verification tool for rom/iso verification, I found very few that were not built
specifically for Windows.

After having written `check-roms` in Go, I thought I would port at least the basics to Rust. I then wrote the initial version
of this tool as one of my first Rust projects, and it was very hard to maintain and too limited and I very much had no idea
what I was doing in the language.

This version uses a better CLI interface and maintains an SQLite database so that you can keep track of things more easily.

Installation
------------

Prebuild binaries are not yet available on the [Releases](https://github.com/sammiq/rcr2/releases) page for Linux, Mac OS and Windows.
See Building below.

Building
--------

You need a working [Rust](https://www.rust-lang.org) installation (I use Rust 1.85 on Linux Mint 22.1).

Build the tool with:

    cargo build --release

IMPORTANT: Performance will be *terrible* without compiling for release, the SHA hash code is incredibly slow when unoptimised.

Usage
-----
```
Usage: rcr2 [OPTIONS] <COMMAND>

Commands:
  database  Perform a database operation
  file      Perform a file operation
  help      Print this message or the help of the given subcommand(s)

Options:
  -d, --database <DATABASE>  Path to the database [default: .rcr.db]
      --debug                Enable debug output
  -h, --help                 Print help
  -V, --version              Print version


Database Commands:
  initialize  Initialize the database
  import      Import data into the database
  search      Search the database
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help


File Commands:
  scan    Scan all files in the directory and store the results in the database
  update  Update files in the database from the directory, checking for new, renamed and removed files
  check   Check all files in the directory against the database
  help    Print this message or the help of the given subcommand(s)

Options:
  -e, --exclude-extensions <EXCLUDE_EXTENSIONS>
          List of file extensions to exclude, comma separated [default: m3u,dat]
  -h, --help
          Print help

```

Limitations
-----------

- Supports only UTF-8 files and paths; this is good enough for my use case and the conversions and storage of other encodings is not straightforward.
- Does not read elements other than `<rom>` inside `<game>` from dat file (I  am yet to find a file containing others).
