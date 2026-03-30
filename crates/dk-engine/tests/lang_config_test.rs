use dk_core::SymbolKind;
use dk_engine::parser::lang_config::default_kind_mapping;

#[test]
fn function_and_method_map_to_function() {
    assert_eq!(default_kind_mapping("function"), Some(SymbolKind::Function));
    assert_eq!(default_kind_mapping("method"), Some(SymbolKind::Function));
}

#[test]
fn class_maps_to_class() {
    assert_eq!(default_kind_mapping("class"), Some(SymbolKind::Class));
}

#[test]
fn struct_maps_to_struct() {
    assert_eq!(default_kind_mapping("struct"), Some(SymbolKind::Struct));
}

#[test]
fn enum_maps_to_enum() {
    assert_eq!(default_kind_mapping("enum"), Some(SymbolKind::Enum));
}

#[test]
fn trait_maps_to_trait() {
    assert_eq!(default_kind_mapping("trait"), Some(SymbolKind::Trait));
}

#[test]
fn impl_maps_to_impl() {
    assert_eq!(default_kind_mapping("impl"), Some(SymbolKind::Impl));
}

#[test]
fn interface_maps_to_interface() {
    assert_eq!(default_kind_mapping("interface"), Some(SymbolKind::Interface));
}

#[test]
fn type_alias_and_type_map_to_type_alias() {
    assert_eq!(default_kind_mapping("type_alias"), Some(SymbolKind::TypeAlias));
    assert_eq!(default_kind_mapping("type"), Some(SymbolKind::TypeAlias));
}

#[test]
fn const_maps_to_const() {
    assert_eq!(default_kind_mapping("const"), Some(SymbolKind::Const));
}

#[test]
fn static_maps_to_static() {
    assert_eq!(default_kind_mapping("static"), Some(SymbolKind::Static));
}

#[test]
fn module_maps_to_module() {
    assert_eq!(default_kind_mapping("module"), Some(SymbolKind::Module));
}

#[test]
fn variable_maps_to_variable() {
    assert_eq!(default_kind_mapping("variable"), Some(SymbolKind::Variable));
}

#[test]
fn unknown_suffix_returns_none() {
    assert_eq!(default_kind_mapping("macro"), None);
    assert_eq!(default_kind_mapping(""), None);
    assert_eq!(default_kind_mapping("unknown"), None);
}
