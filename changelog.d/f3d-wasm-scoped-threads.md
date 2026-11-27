### Fixed

- Decode a Fusion design with design history on `wasm32-unknown-unknown`, where the decoder's scoped threads could not start and stopped the whole decode. Each task now runs in turn on that target, and the result is the same.
