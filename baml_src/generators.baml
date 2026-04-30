// Generators: who/what should baml-cli emit when `baml-cli generate` runs.
//
// dkod emits TWO clients from the same agent/workflow graph:
//   1. Rust  → consumed by dk-runner / dk-agent-sdk / dk-server in-process.
//   2. TypeScript → consumed by clickhouse-monitoring via git submodule
//      (see baml_client_ts/README.md). Gives the Next.js dashboard typed
//      BAML calls with identical signatures, plus streamGenerator() for
//      partial-struct UI rendering.
//
// Source of truth is always `baml_src/`; regenerate both clients with
// `baml-cli generate` (or `just generate` once the xtask lands).
generator rust {
    // Valid values: "python/pydantic", "typescript", "go", "rust", "ruby/sorbet", "rest/openapi"
    output_type "rust"

    // Relative to baml_src/, so "../" emits to `<repo>/baml_client/`.
    // Rust output stays gitignored — regenerated from CI / cargo xtask.
    output_dir "../"

    // Must match the `baml` crate version in Cargo.toml.
    version "0.221.0"
}

generator typescript {
    output_type "typescript"

    // Relative to baml_src/, so "../" emits to `<repo>/baml_client/`
    // (TS generator places its output under a `baml_client/` subdir too).
    // We rename the output directory after generation to `baml_client_ts/`
    // so it doesn't collide with the Rust module, then commit it so a
    // downstream repo (clickhouse-monitoring) can consume it via git
    // submodule without needing baml-cli installed.
    output_dir "../baml_client_ts_raw/"

    // Must match the @boundaryml/baml npm package version pinned in the
    // consuming clickhouse-monitoring package.json.
    version "0.221.0"
}
