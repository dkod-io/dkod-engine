use dk_core::{CallKind, SymbolKind, Visibility};
use dk_engine::parser::ParserRegistry;
use std::path::Path;

#[test]
fn test_extract_java_classes_and_interfaces() {
    let registry = ParserRegistry::new();
    let source = br#"
public class UserService {
    private String name;

    public UserService(String name) {
        this.name = name;
    }

    public void processRequest(Request req) {
        validate(req);
    }

    private void validate(Request req) {
        // internal validation
    }
}

public interface AuthProvider {
    boolean authenticate(String token);
}

public enum Status {
    ACTIVE,
    INACTIVE,
    PENDING
}
"#;
    let analysis = registry
        .parse_file(Path::new("UserService.java"), source)
        .unwrap();

    let names: Vec<&str> = analysis.symbols.iter().map(|s| s.name.as_str()).collect();

    // Class
    assert!(
        names.contains(&"UserService"),
        "Missing UserService class, got: {:?}",
        names
    );
    let user_svc = analysis
        .symbols
        .iter()
        .find(|s| s.name == "UserService")
        .unwrap();
    assert_eq!(user_svc.kind, SymbolKind::Class);
    assert_eq!(user_svc.visibility, Visibility::Public);

    // Interface
    assert!(
        names.contains(&"AuthProvider"),
        "Missing AuthProvider interface, got: {:?}",
        names
    );
    let auth_provider = analysis
        .symbols
        .iter()
        .find(|s| s.name == "AuthProvider")
        .unwrap();
    assert_eq!(auth_provider.kind, SymbolKind::Interface);
    assert_eq!(auth_provider.visibility, Visibility::Public);

    // Enum
    assert!(
        names.contains(&"Status"),
        "Missing Status enum, got: {:?}",
        names
    );
    let status = analysis
        .symbols
        .iter()
        .find(|s| s.name == "Status")
        .unwrap();
    assert_eq!(status.kind, SymbolKind::Enum);
    assert_eq!(status.visibility, Visibility::Public);

    // Methods
    assert!(
        names.contains(&"processRequest"),
        "Missing processRequest method, got: {:?}",
        names
    );
    let process_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "processRequest")
        .unwrap();
    assert_eq!(process_fn.kind, SymbolKind::Function);
    assert_eq!(process_fn.visibility, Visibility::Public);

    let validate_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "validate")
        .unwrap();
    assert_eq!(validate_fn.kind, SymbolKind::Function);
    assert_eq!(validate_fn.visibility, Visibility::Private);

    // Constructor
    let constructor = analysis
        .symbols
        .iter()
        .filter(|s| s.name == "UserService" && s.kind == SymbolKind::Function)
        .count();
    assert!(
        constructor >= 1,
        "Missing UserService constructor, got: {:?}",
        names
    );
}

#[test]
fn test_extract_java_visibility() {
    let registry = ParserRegistry::new();
    let source = br#"
public class Config {
    public void publicMethod() {}
    protected void protectedMethod() {}
    private void privateMethod() {}
    void packagePrivateMethod() {}
}
"#;
    let analysis = registry
        .parse_file(Path::new("Config.java"), source)
        .unwrap();

    let public_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "publicMethod")
        .unwrap();
    assert_eq!(public_fn.visibility, Visibility::Public);

    let protected_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "protectedMethod")
        .unwrap();
    assert_eq!(
        protected_fn.visibility,
        Visibility::Public,
        "protected should map to Public"
    );

    let private_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "privateMethod")
        .unwrap();
    assert_eq!(private_fn.visibility, Visibility::Private);

    let pkg_fn = analysis
        .symbols
        .iter()
        .find(|s| s.name == "packagePrivateMethod")
        .unwrap();
    assert_eq!(
        pkg_fn.visibility,
        Visibility::Private,
        "package-private should map to Private"
    );
}

#[test]
fn test_extract_java_calls() {
    let registry = ParserRegistry::new();
    let source = br#"
public class Main {
    public void run() {
        UserService service = new UserService("admin");
        service.processRequest(req);
        validate(req);
        System.out.println("done");
    }
}
"#;
    let analysis = registry
        .parse_file(Path::new("Main.java"), source)
        .unwrap();

    let call_names: Vec<&str> = analysis
        .calls
        .iter()
        .map(|c| c.callee_name.as_str())
        .collect();

    // Constructor: new UserService(...)
    assert!(
        call_names.contains(&"UserService"),
        "Expected UserService constructor call in {:?}",
        call_names
    );

    // Direct method call: validate()
    assert!(
        call_names.contains(&"validate"),
        "Expected validate in {:?}",
        call_names
    );

    // Method invocation: service.processRequest()
    assert!(
        call_names.contains(&"processRequest"),
        "Expected processRequest in {:?}",
        call_names
    );

    // Check call kinds
    let constructor_call = analysis
        .calls
        .iter()
        .find(|c| c.callee_name == "UserService")
        .unwrap();
    assert_eq!(constructor_call.kind, CallKind::DirectCall);
}

#[test]
fn test_extract_java_imports() {
    let registry = ParserRegistry::new();
    let source = br#"
import java.util.List;
import java.util.Map;
import static java.lang.Math.PI;
import com.example.service.UserService;

public class App {
}
"#;
    let analysis = registry
        .parse_file(Path::new("App.java"), source)
        .unwrap();

    assert!(
        analysis.imports.len() >= 4,
        "Expected at least 4 imports, got: {} => {:?}",
        analysis.imports.len(),
        analysis
            .imports
            .iter()
            .map(|i| format!("{}:{}", i.module_path, i.imported_name))
            .collect::<Vec<_>>()
    );

    // java.util.List
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("java.util.List")),
        "Should have import java.util.List"
    );

    // com.example.service.UserService
    assert!(
        analysis
            .imports
            .iter()
            .any(|i| i.module_path.contains("UserService")),
        "Should have import UserService"
    );
}

#[test]
fn test_registry_supports_java() {
    let registry = ParserRegistry::new();
    assert!(registry.supports_file(Path::new("Main.java")));
    assert!(registry.supports_file(Path::new("UserService.java")));
}
