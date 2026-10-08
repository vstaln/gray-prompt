# gray-prompt

Layered prompt customization: global ~/.gray/prompt/custom.md + per-project .gray-prompt.md (port of pi's prompt-customizer).

A sidecar plugin for [gray](https://github.com/vstaln/gray), scaffolded by
[gray-account](https://github.com/vstaln/gray-account).

## Install

```sh
gray plugin install prompt
```

## Develop

```sh
cargo test
gray account check      # entry point + manifest handshake
gray account publish    # check → build → release → publish to the gray registry
```

Bump `version` in `Cargo.toml` before each `publish`; the registry refuses to
republish a version.
