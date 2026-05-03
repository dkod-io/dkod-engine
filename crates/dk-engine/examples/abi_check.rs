//! Diagnostic: print the tree-sitter ABI version of every registered grammar.

use dk_engine::parser::lang_config::LanguageConfig;
use dk_engine::parser::langs;

fn check(name: &str, lang: tree_sitter::Language) {
    println!("{:>12}  ABI {}", name, lang.version());
}

fn main() {
    check("rust", langs::rust::RustConfig.language());
    check("python", langs::python::PythonConfig.language());
    check("go", langs::go::GoConfig.language());
    check("java", langs::java::JavaConfig.language());
    check("cpp", langs::cpp::CppConfig.language());
    check("csharp", langs::csharp::CSharpConfig.language());
    check("ruby", langs::ruby::RubyConfig.language());
    check("php", langs::php::PhpConfig.language());
    check("swift", langs::swift::SwiftConfig.language());
    check("scala", langs::scala::ScalaConfig.language());
    check("haskell", langs::haskell::HaskellConfig.language());
    check("julia", langs::julia::JuliaConfig.language());
    check("bash", langs::bash::BashConfig.language());
    check("kotlin", langs::kotlin::KotlinConfig.language());
    check("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into());
    check("tsx", tree_sitter_typescript::LANGUAGE_TSX.into());
}
