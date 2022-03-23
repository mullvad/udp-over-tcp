# Changelog
All changes to the software that can be noticed from the users' perspective should have an entry in
this file. Except very minor things that will not affect functionality.

### Format

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/).

Entries should have the imperative form, just like commit messages. Start each entry with words like
add, fix, increase, force etc.. Not added, fixed, increased, forced etc.

Line wrap the file at 100 chars.                                              That is over here -> |

### Categories each change fall into

* **Added**: for new features.
* **Changed**: for changes in existing functionality.
* **Deprecated**: for soon-to-be removed features.
* **Removed**: for now removed features.
* **Fixed**: for any bug fixes.
* **Security**: in case of vulnerabilities.


## [Unreleased]


## [0.2.0] - 2022-03-23
### Added
- Add a helper build script (`build-static-bins.sh`) to build the binaries
  as statically linked executables on Linux
- Add TCP recv timeout support. Allows setting an upper limit on how long
  the library/program will wait for data on the TCP socket before closing
  the connection.

### Fixed
- Destroy all sockets when Udp2Tcp::run future is dropped
- Move socket buffers to the heap to avoid crashes on machines with too small
  stack sizes in debug builds.

... And more. We never really cut any 0.1.0 release

## [0.1.0] - ?
There never was a real 0.1.0 release. We just kept using the main branch commits
