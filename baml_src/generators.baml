// Generators: who/what should baml-cli emit when `baml-cli generate` runs.
//
// dkod uses the Rust generator so dk-runner / dk-agent-sdk / dk-server can
// import the typed agent & workflow clients from the generated `baml_client`
// module.
generator target {
    // Valid values: "python/pydantic", "typescript", "go", "rust", "ruby/sorbet", "rest/openapi"
    output_type "rust"

    // Where the generated code will be saved (relative to baml_src/).
    // Output lands in `<repo>/baml_client/` — add that path to .gitignore
    // and regenerate from CI / a cargo xtask / a dev `just generate` step.
    output_dir "../"

    // Must match the `baml` crate version in Cargo.toml.
    version "0.221.0"
}
