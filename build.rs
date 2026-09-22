use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing manifest path"));
    let library_root = manifest.join("lib");
    println!("cargo:rerun-if-changed={}", library_root.display());

    let mut modules = BTreeMap::new();
    collect_modules(&library_root, &library_root, &mut modules);

    let mut generated = String::from("pub const STANDARD_LIBRARY: &[(&str, &str)] = &[\n");
    for (name, path) in modules {
        generated.push_str(&format!("    ({name:?}, include_str!({:?})),\n", path));
    }
    generated.push_str("];\n");

    let output = PathBuf::from(env::var("OUT_DIR").expect("missing output path"));
    fs::write(output.join("standard_library.rs"), generated)
        .expect("failed to write standard library map");
}

fn collect_modules(root: &Path, directory: &Path, modules: &mut BTreeMap<String, PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("failed to read library entry").path();
        if path.is_dir() {
            collect_modules(root, &path, modules);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("luau") {
            continue;
        }

        let relative = path.strip_prefix(root).expect("library path escaped root");
        let mut name = relative.with_extension("");
        if name.file_name().and_then(|part| part.to_str()) == Some("init") {
            name.pop();
        }
        let name = name
            .to_str()
            .expect("library module path must be valid UTF-8")
            .replace(std::path::MAIN_SEPARATOR, "/");
        assert!(!name.is_empty(), "lib/init.luau is not a valid module name");
        assert!(
            modules.insert(name.clone(), path.clone()).is_none(),
            "duplicate standard library module {name:?}"
        );
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
