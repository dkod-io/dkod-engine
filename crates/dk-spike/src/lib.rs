//! Backend crate spike — evaluates candidate dependencies for integration.
//!
//! Each probe is a zero-op that forces the crate into the compile graph so we
//! can assert it builds in-workspace without touching production code.
#![allow(dead_code)]

fn _probe_octocrab() {
    let _ = octocrab::instance;
}

fn _probe_enum_dispatch() {
    #[enum_dispatch::enum_dispatch]
    trait Speak {
        fn hello(&self) -> &'static str;
    }
    struct Dog;
    impl Speak for Dog {
        fn hello(&self) -> &'static str {
            "woof"
        }
    }
    struct Cat;
    impl Speak for Cat {
        fn hello(&self) -> &'static str {
            "meow"
        }
    }
    #[enum_dispatch::enum_dispatch(Speak)]
    enum Animal {
        Dog,
        Cat,
    }
    let _ = Animal::Dog(Dog).hello();
}

fn _probe_typetag() {
    #[typetag::serde(tag = "kind")]
    trait Event: std::fmt::Debug {
        fn name(&self) -> &'static str;
    }
    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    struct Ping;
    #[typetag::serde]
    impl Event for Ping {
        fn name(&self) -> &'static str {
            "ping"
        }
    }
}

fn _probe_tracing_core() {
    let _ = tracing_core::Level::INFO;
}

fn _probe_chdb_rust() {
    // chDB is an embedded ClickHouse. Links against `libchdb.so` (installed
    // to /usr/local/lib via `curl -sL https://lib.chdb.io | bash`). The
    // `chdb-rust` crate is git-only (chdb-io/chdb-rust); the published
    // `chdb` crate on crates.io is broken. We just reference a type so
    // linkage is verified without running a query.
    let _ = std::marker::PhantomData::<chdb_rust::format::OutputFormat>;
}
