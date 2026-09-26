use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

/// A Vite frontend that build.rs compiles (when npm is available) and embeds into the binary.
struct Frontend {
    /// Directory name under the manifest dir (also used in env flag names).
    dir: &'static str,
    /// Env suffix: MAFIA_SKIP_<ENV>_BUILD / MAFIA_FORCE_<ENV>_BUILD.
    env: &'static str,
    /// Generated file name in OUT_DIR.
    generated: &'static str,
    /// Struct and static names written into the generated file.
    struct_name: &'static str,
    const_name: &'static str,
    /// Human-readable label for messages.
    label: &'static str,
}

const FRONTENDS: &[Frontend] = &[
    Frontend {
        dir: "activity",
        env: "ACTIVITY",
        generated: "activity_static.rs",
        struct_name: "EmbeddedActivityAsset",
        const_name: "ACTIVITY_ASSETS",
        label: "Activity UI",
    },
    Frontend {
        dir: "casino-web",
        env: "CASINO",
        generated: "casino_static.rs",
        struct_name: "EmbeddedCasinoAsset",
        const_name: "CASINO_ASSETS",
        label: "Casino UI",
    },
    Frontend {
        dir: "stocks-web",
        env: "STOCKS",
        generated: "stocks_static.rs",
        struct_name: "EmbeddedStocksAsset",
        const_name: "STOCKS_ASSETS",
        label: "Stocks UI",
    },
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    for frontend in FRONTENDS {
        let frontend_dir = manifest_dir.join(frontend.dir);
        let dist_dir = frontend_dir.join("dist");

        println!(
            "cargo:rerun-if-env-changed=MAFIA_SKIP_{}_BUILD",
            frontend.env
        );
        println!(
            "cargo:rerun-if-env-changed=MAFIA_FORCE_{}_BUILD",
            frontend.env
        );
        for file in [
            "package.json",
            "package-lock.json",
            "index.html",
            "src",
            "dist",
        ] {
            println!("cargo:rerun-if-changed={}/{file}", frontend.dir);
        }

        if should_build(frontend, &dist_dir) {
            build_frontend(frontend, &frontend_dir, &dist_dir);
        }

        if !dist_dir.join("index.html").is_file() {
            panic!(
                "{}/dist/index.html missing. Run `cd {} && npm ci && npm run build`, \
                 or build with npm available so build.rs can embed the {}.",
                frontend.dir, frontend.dir, frontend.label
            );
        }

        let generated = out_dir.join(frontend.generated);
        write_static(frontend, &dist_dir, &generated).unwrap();
    }
}

fn should_build(frontend: &Frontend, dist_dir: &Path) -> bool {
    if env::var_os(format!("MAFIA_SKIP_{}_BUILD", frontend.env)).is_some() {
        return false;
    }
    env::var_os(format!("MAFIA_FORCE_{}_BUILD", frontend.env)).is_some()
        || !dist_dir.join("index.html").is_file()
        || source_changed(dist_dir)
}

fn build_frontend(frontend: &Frontend, frontend_dir: &Path, dist_dir: &Path) {
    if !frontend_dir.join("package.json").is_file() {
        return;
    }

    if !frontend_dir.join("node_modules").is_dir() {
        run_npm(frontend_dir, &["ci"]);
    }
    run_npm(frontend_dir, &["run", "build"]);

    if !dist_dir.join("index.html").is_file() {
        panic!(
            "{} build finished but {}/dist/index.html was not created.",
            frontend.label, frontend.dir
        );
    }
}

fn run_npm(frontend_dir: &Path, args: &[&str]) {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm)
        .args(args)
        .current_dir(frontend_dir)
        .status()
        .unwrap_or_else(|err| panic!("failed to run `{npm} {}`: {err}", args.join(" ")));

    if !status.success() {
        panic!("`{npm} {}` failed with status {status}.", args.join(" "));
    }
}

fn source_changed(dist_dir: &Path) -> bool {
    let Some(frontend_dir) = dist_dir.parent() else {
        return false;
    };
    let Ok(dist_modified) = fs::metadata(dist_dir.join("index.html")).and_then(|m| m.modified())
    else {
        return true;
    };

    [
        frontend_dir.join("package.json"),
        frontend_dir.join("package-lock.json"),
        frontend_dir.join("index.html"),
        frontend_dir.join("src"),
    ]
    .iter()
    .any(|path| path_newer_than(path, dist_modified))
}

fn path_newer_than(path: &Path, time: std::time::SystemTime) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.is_file() {
        return metadata.modified().is_ok_and(|modified| modified > time);
    }
    if !metadata.is_dir() {
        return false;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .any(|entry| path_newer_than(&entry.path(), time))
}

fn write_static(frontend: &Frontend, dist_dir: &Path, generated: &Path) -> io::Result<()> {
    let mut entries = Vec::new();
    collect_files(dist_dir, dist_dir, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut file = fs::File::create(generated)?;
    writeln!(
        file,
        "pub struct {} {{ pub path: &'static str, pub content_type: &'static str, pub body: &'static [u8] }}",
        frontend.struct_name
    )?;
    writeln!(
        file,
        "pub static {}: &[{}] = &[",
        frontend.const_name, frontend.struct_name
    )?;
    for (url_path, fs_path) in entries {
        writeln!(
            file,
            "    {} {{ path: {:?}, content_type: {:?}, body: include_bytes!({:?}) }},",
            frontend.struct_name,
            url_path,
            content_type(&url_path),
            fs_path.to_string_lossy()
        )?;
    }
    writeln!(file, "];")?;
    Ok(())
}

fn collect_files(root: &Path, current: &Path, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            let relative = path.strip_prefix(root).unwrap();
            let url_path = format!(
                "/{}",
                relative
                    .iter()
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            );
            out.push((url_path, path));
        }
    }
    Ok(())
}

fn content_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}
