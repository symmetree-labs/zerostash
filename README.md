# Zerostash

![master](https://github.com/rsdy/zerostash/workflows/Rust/badge.svg?branch=master)

Zerostash is a deduplicated, encrypted file store with versioning.

It was designed for speed and to secure all metadata related to the
files, including the exact size of the data that is stored.

On a M1 Macbook Air, Zerostash can achieve speeds of around 1GB/s.

## Use cases

 * Incremental backups in the cloud, or on external hard drives
 * Encrypt and store entire workspaces for fast sync between computer
 * Git on crypto

## Getting started

Once you install the `0s` command using one of the methods below, you
can start backing up:

    0s commit /path/to/repository $(pwd)
	
That's it! No configuration necessary.

You will be asked for a username and a password to create a stash,
which you'll need to enter on any subsequent invocations.

If you want to get fancy, you can leave a note with your commit, just
like you do with Git.

    0s commit -m 'My first backup!' /path/to/repository $(pwd)

Commits are only created if there are changes between runs to preserve
space, and speed things up.

You can then restore your backups using the `checkout` subcommand and
entering your credentials:

	0s checkout /path/to/repository files_to_restore/*

For more details, run

    0s --help

## Configuration

An example config file can be found [here](./config.toml.example).
Place it at `~/.config/zerostash/config.toml`, or inside your
`$XDG_CONFIG_HOME/zerostash` directory.

Using a configuration file is optional, but can make managing stashes easier.

An example config looks like so:

    [mystash]
    key = { source = "ask"}
    backend = { type = "fs", path = "/archive" }

To use your newly defined `mystash` stash in your backups, just use it
instead of a path to the repository.

	0s commit mystash /path/to/movies

## Installation

Zerostash works on Linux, macOS, and Windows, and you can download
[pre-built binaries](https://github.com/rsdy/zerostash/releases)!

If you're looking for package manager integrations, though, look below.

### Installation on macOS

There is a homebrew tap you can use!

    brew install symmetree-labs/homebrew-tap/zerostash
	
### Installation on NixOS

This repo is actually a nix flake! You can include the `zerostash`
package in your flake-based configurations, or just run it like this:

	nix run github:symmetree-labs/zerostash
	
Note: nix/macOS currently is not supported due to a [known
issue](https://github.com/NixOS/nixpkgs/issues/86299). Please help us find a workaround!

### Install with cargo

Assuming you have `cargo` installed on your system, you can use it to install zerostash from [crates.io](https://crates.io).

    cargo install zerostash

### Using pre-built binaries

You can download a static Linux binary from the [GitHub
Releases](https://github.com/rsdy/zerostash/releases) page.

Place it in your `$PATH`, and then run:

    0s --help

### Build from source

The usual Rust incantation will also do to build the binary
yourself. Use [`rustup`](https://rustup.rs/) to get `cargo` running or
use your package manager, then off you go:

    cargo build --release

## Threat model

Zerostash considers the following things to be part of the threat model:

 * Protect data confidentiality, integrity and authenticity
 * The exact size of data should not be known
 * Individual user data shouldn't be attributable on shared storage
 * Once a data is shared, it is no longer secure.
 * Deleting data from the storage should be possible
 * Access to only the key and raw data should not be sufficient for
   full data compromise

## Design

For more details about the cryptographic design, consult the
[documentation](https://github.com/symmetree-labs/infinitree/blob/main/DESIGN.md)
in the underlying
[Infinitree](https://github.com/symmetree-labs/infinitree) library.

## Security notice

**This is unreviewed security software. Use at your own risk.**

## License

Distributed under GPLv3.
