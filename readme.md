# EW
(stands for ew why)

A (mostly functioning) server for Love Live! School idol festival 2 MIRACLE LIVE!

## Building

### Requirements
- [rust](https://www.rust-lang.org/tools/install)

### Packaging/Running

**Build:**
Debug: `cargo run`
Release: `cargo build --release`

**Docker:**
`docker build --tag ew --file docker/Dockerfile .`
Will create docker image with tag "ew"
