import re

with open('crates/dk-engine/src/graph/symbols.rs', 'r') as f:
    content = f.read()
content = content.replace("self.upsert_symbols(repo_id, &[sym.clone()]).await", "self.upsert_symbols(repo_id, std::slice::from_ref(sym)).await")
with open('crates/dk-engine/src/graph/symbols.rs', 'w') as f:
    f.write(content)

with open('crates/dk-engine/src/graph/types.rs', 'r') as f:
    content = f.read()
content = content.replace("self.upsert_type_infos(&[info.clone()]).await", "self.upsert_type_infos(std::slice::from_ref(info)).await")
with open('crates/dk-engine/src/graph/types.rs', 'w') as f:
    f.write(content)

with open('crates/dk-engine/src/graph/callgraph.rs', 'r') as f:
    content = f.read()
content = content.replace("self.insert_edges(&[edge.clone()]).await", "self.insert_edges(std::slice::from_ref(edge)).await")
with open('crates/dk-engine/src/graph/callgraph.rs', 'w') as f:
    f.write(content)

with open('crates/dk-engine/src/git/repository.rs', 'r') as f:
    content = f.read()
content = content.replace("let entry_oid = entry.oid().to_hex().to_string();", "let _entry_oid = entry.oid().to_hex().to_string();")
with open('crates/dk-engine/src/git/repository.rs', 'w') as f:
    f.write(content)
