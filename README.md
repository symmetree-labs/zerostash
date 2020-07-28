# Zerostash

![master](https://github.com/rsdy/zerostash/workflows/Rust/badge.svg?branch=master)

## Wat dis

Zerostash is a deduplicated, encrypted data store that provides native
versioning capabilities, and was designed to secure all metadata
related to the files, including the exact size of the data that is
stored in the containers.

## Use cases

 * Versioned backups in the cloud, or external hard drives
 * Encrypt and store entire workspaces for fast sync between computer
 * Easily sync & wipe data & encryption programs while travelling
 * Storage and sync backend for offsite backups

## Threat model

Zerostash considers the following things to be part of the threat model:

 * Protect data confidentiality, integrity and authenticity
 * The exact size of data should not be known
 * Individual user data shouldn't be attributable on shared storage
 * Once a data is shared, it is no longer secure.
 * Deleting data from the storage should be possible
 * Access to only the key and raw data should not be sufficient for
   full data compromise

## How to

You can download a static Linux binary from the [GitHub Releases](https://github.com/rsdy/zerostash/releases) page.
Place it in your `$PATH`, and then run:

    0s help
    
An example config file can be found [here](./config.toml.example).
Place it in `$XDG_CONFIG_HOME/zerostash/config.toml`, and edit as needed.
On most systems, this will be at `~/.config/zerostash/config.toml`

Using a configuration file is optional, but can make managing stashes easier.

**Expect some commands to be useless**. This is highly experimental software, and functionality is missing.
At least the following commands **will** work:

 * `wipe`: destroy local stash of files
 * `ls`: list files in a stash
 * `commit`: add new files to a stash
 * `checkout`: restore files

## Build
	
The usual Rust incantation will also do to build the binary yourself.
A nightly compiler is needed for a few dependencies.

    cargo +nightly build --release
    
To get help on usage, try:

    cargo +nightly run --release --bin 0s help

## Benchmarks

At the moment, this is a non-functional demonstration of the object
format. You can do something like so:

    cargo run --release --bin 0s-bench 4 $(pwd) ../repo ../restore

So the process will use 4 threads to back up the current directory to
`../repo`, and restore it immediately after to `../restore`.

Some extremely unscientific measurements on my desktop about performance:

```
 * files: 9917,
 * chunks: 12490,
 * data size: 1071.7888813018799
 * throughput: 356.81619052498746
 * objects: 161
 * output size: 644
 * compression ratio: 0.6008646023811578
 * meta dump time: 0.088976801
 * meta object count: 1

read time: 0.193175887
restore time: 1.5405925969999998
throughput packed: 371.4452107897469
throughput unpacked: 618.1845449336705
```

## Design

For more details about the design, consult the [documentation](./docs/design.md).

## License

Distributed under GPLv3.
