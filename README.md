# mixosc
OSC Mixer in Rust

Minimal X32 connection monitor.

`mixosc` exposes:

- A library for X32 reference-file loading and UDP connection probing.
- A GUI program that discovers an X32 on the local network and shows `connected` or `disconnected`.

Run it with automatic discovery:

```bash
cargo run
```

Or override discovery with a mixer address:

```bash
cargo run -- 192.168.1.62
```

Or via environment variable:

```bash
MIXOSC_MIXER_ADDR=192.168.1.62 cargo run
```

The reference JSON files stay in `~/Files/OSC` and are loaded from there on demand.
