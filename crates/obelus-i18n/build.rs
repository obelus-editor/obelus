//! Reads `locales/`, checks every language against the first, and writes a
//! function per message into `OUT_DIR` -- see `build/generate.rs`.

#[path = "build/generate.rs"]
mod generate;

use std::path::Path;

/// The languages, by directory, the one the others are measured against
/// first. The order is the catalogue's, and `Language` in the crate is held
/// to it by a test.
const LANGUAGES: &[&str] = &["en", "zh-CN"];

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let locales = root.join("locales");
    println!("cargo::rerun-if-changed=locales");
    let mut wrong = Vec::new();
    // A directory nobody listed is a translation nobody can choose, which
    // is as much a mistake as a missing one.
    if let Ok(entries) = std::fs::read_dir(&locales) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !LANGUAGES.contains(&name.as_str()) {
                wrong.push(format!(
                    "locales/{name}: not one of the languages build.rs lists"
                ));
            }
        }
    }
    let mut languages = Vec::new();
    for word in LANGUAGES {
        let directory = locales.join(word);
        println!("cargo::rerun-if-changed=locales/{word}");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            wrong.push(format!("locales/{word}: missing"));
            continue;
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "ftl") {
                continue;
            }
            let Some(stem) = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
            else {
                continue;
            };
            println!("cargo::rerun-if-changed=locales/{word}/{stem}.ftl");
            match std::fs::read_to_string(&path) {
                Ok(source) => files.push(generate::File {
                    path: format!("locales/{word}/{stem}.ftl"),
                    stem,
                    source,
                }),
                Err(error) => wrong.push(format!("locales/{word}/{stem}.ftl: {error}")),
            }
        }
        languages.push(generate::Language {
            word: (*word).to_string(),
            files,
        });
    }
    let generated = match generate::generate(&languages) {
        Ok(generated) if wrong.is_empty() => generated,
        Ok(_) => String::new(),
        Err(more) => {
            wrong.extend(more);
            String::new()
        }
    };
    if !wrong.is_empty() {
        for line in &wrong {
            println!("cargo::error={line}");
        }
        return;
    }
    let out = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR for a build script");
    std::fs::write(Path::new(&out).join("messages.rs"), generated)
        .expect("the build's own directory takes a file");
}
