//! Embed the same pinned component icons as native, without CDN timing in tests.
use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let icons = PathBuf::from(
        env::var("DEP_GPUI_COMPONENT_DEFAULT_ICONS_ICONS_DIR")
            .expect("gpui-component-assets must publish its icon directory"),
    );
    println!("cargo:rerun-if-changed={}", icons.display());
    let mut paths: Vec<_> = fs::read_dir(&icons)
        .expect("read pinned icon directory")
        .map(|entry| entry.expect("read icon entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "svg"))
        .collect();
    paths.sort();
    let mut source = String::from("static ICONS: &[(&str, &[u8])] = &[\n");
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 icon filename");
        writeln!(
            source,
            "({:?}, include_bytes!({:?})),",
            format!("icons/{name}"),
            path
        )
        .expect("write generated source");
    }
    source.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("icons.rs"),
        source,
    )
    .expect("write embedded icon table");
}
