# Zerostash

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

## Design

Zerostash is designed to be portable and easy to implement in various
programming languages and operating systems. It aims to be fast and
correct, over providing more complex features at the cost of
complexity.

Zerostash organizes user data into *stashes*. Stashes are the root of
trust for all objects referenced and stored by them, including keying
information. The encryption key for stash can be derived directly from
the user passphrase using Argon2.

Data is stored in uniform *objects*, with a hard-coded size of
4MB. This is to mask the exact size of the data.  An *object id* is 32
bytes of random represented in base32.  Objects are padded with random
bytes when they are not fully utilised.

Zerostash distinguishes 2 types of objects, with differing internal
structure:

 * Metadata objects
 * Data objects
 
Parallelism is important for speed, and objects and their data are
optimised for parallel access. Unlike most similar programs, Zerostash
can't rely on the filesystem to do the synchronization of file
operations, therefore much of it needs to happen in application logic.

### Metadata objects

Metadata objects(mobject) are general purpose stores which are
encrypted as a whole.

A mobject stores several *records* that may contain arbitrary
serialized of data. Following a 512-byte header (padded with 0), a
mobject stores records as LZ4 streams with 64k block size, with
records aligned to multiples of 64k.

The header contains a list of records and their offset from the start
of the file, serialized using msgpack. The 512-byte header limits the 
number of records a mobject can contain, which looks like a reasonable
tradeoff at this time. In the future, the header can be extended in a
backwards compatible way.

This is what a metadata object looks like:

| Offset         | Content                               |
| ------         | -------                               |
| 0              | msgpack-encoded header                   |
| `512`          | `LZ4_stream(field 1)`                 |
| `512 + 64k`    | `... LZ4_stream(field 1) + 0 padding` |
| `512 + 128k`   | `LZ4_stream(field 2)`                 |
| `512 + 192k`   | `LZ4_stream(field 2)`                 |
| `512 + 256k`   | `LZ4_stream(field 2)`                 |
| `... 4M - 64k` | `... LZ4(field N) + random padding`   |

Mobjects are currently used to store all indexing information for data
objects, as well as internal state for more efficient operation.

The recognised fields currently are:

 * File list
 * Chunk list
 
Access to different fields can be parallelised through `mmap`-ing
mobjects, however, the exact length of a field without padding is
_not_ stored. It is expected that the LZ4 stream is terminated
using the correct framing provided by `liblz4`.

Metadata objects are encrypted using ChaCha20-Poly1305 and a subkey
derived from the user passphrase. The root object id of a stash is
also derived from the same passphrase.

### Data objects

A data object (dobject) is a series of *chunks* that are individually LZ4
compressed, then encrypted using a symmetric ChaCha20-Poly1305 AED
construction.

Dobjects are tightly packed, and padded at the end of the file
with random bytes.

```
| ChaCha-Poly(LZ4(chunk 1)) | ChaCha-Poly(LZ4(chunk 2)) |
|               ChaCha-Poly(LZ4(chunk 3))               | 
|           ... ChaCha-Poly(LZ4(chunk 3))               | 
| ChaCha-Poly(LZ4(chunk 4)) | random padding            | 
```

In order to extract chunks from dobjects, the following needs to be known:

 * object id
 * start offset
 * compressed chunk size
 * Blake2s hash of plaintext

The key to encrypt each chunk is `Blake2s(plaintext) XOR Argon2(user
key)`, therefore compromising a user key in itself does not necessarily
result in full data compromise without access to indexing metadata.

## Key management

The user passphrase is the root of trust for a stash. The raw key
material is derived from the user passphrase using Argon2.

To separate the keys used to derive the root object id, encrypt
metadata, and encrypt data, 3 separate subkeys are derived using
libsodium's [key
derivation](https://libsodium.gitbook.io/doc/key_derivation) APIs,
which uses Blake2 under the hood.

## Threat model

Looking at the threat model from the perspective of the following
attacker profiles:

 * Passive storage observer
 * Active storage compromise
 * Full client compromise (User)
 * Full client compromise (Administrator)
 
### Passive storage observer

A passive observer of the storage activity cannot create new objects,
but observe user activity on the storage.

A passive observer will be able to observe the amount of traffic a
user generates, and the objects they access in the duration of the
compromise.

A passive observer may be able to identify individual users based on
traffic correlation or by unique connection identifiers, such as IP
address.

### Active storage compromise

An active adversary on the storage can create new objects and modify
existing ones, plus monitor user activity.

An active adversary can overwrite stored objects in part or in whole,
in a targeted manner.

However, since they don't possess user keys, these modifications can
be detected by a user agent. In effect, the attack would be a DoS,
where an adversary can destroy data selectively.

### Full client compromise (User)

The client will possess all key information, storage provider
credentials, and access details they can intercept, using e.g. a
keylogger. THey, however, will not have access to key material stored
in memory unless they can force the user agent to dump state into an
accessible location.

### Full client compromise (Administrator)

The client will possess all key information, storage provider
credentials, and access details. Any locally stored object is safe
until the user unlocks the metadata referencing it. Once a stash is
opened on a client, the adversary will have read-write access to all
accessible data, local or remote.

## Deduplication

Zerostash uses deduplication of chunks to minimise the storage
use. Currently the algorithm is based on SeaHash, and is tuned towards
creating fewer chunks. I strongly suspect it needs some more effort to
fine-tune the performance.

Currently we create a split when the lower 13 bits of output of
SeaHash is 1. Running with this setting on the repo itself, I get
around 64k big chunks on average, and 10% re-use. I have not math'd
this out properly, but seemed reasonable enough.


## Portability

Zerostash is written in Rust to be easily portable across platforms. Rust can be
easily compiled to static binaries, which can be shared on e.g. a cloud
storage.

One aim is to require no installation or modification to an existing
operating system, although an installation package could provide
platform-specific integration for better user experience.

## License

Distributed under GPLv3.
