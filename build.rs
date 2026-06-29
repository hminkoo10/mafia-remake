use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let activity_dir = manifest_dir.join("activity");
    let dist_dir = activity_dir.join("dist");

    println!("cargo:rerun-if-env-changed=MAFIA_SKIP_ACTIVITY_BUILD");
    println!("cargo:rerun-if-env-changed=MAFIA_FORCE_ACTIVITY_BUILD");
    println!("cargo:rerun-if-changed=activity/package.json");
    println!("cargo:rerun-if-changed=activity/package-lock.json");
    println!("cargo:rerun-if-changed=activity/index.html");
    println!("cargo:rerun-if-changed=activity/src");
    println!("cargo:rerun-if-changed=activity/dist");

    if should_build_activity(&dist_dir) {
        build_activity(&activity_dir, &dist_dir);
    }

    if !dist_dir.join("index.html").is_file() {
        panic!(
            "activity/dist/index.html missing. Run `cd activity && npm ci && npm run build`, \
             or build with npm available so build.rs can embed the Activity UI."
        );
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let generated = out_dir.join("activity_static.rs");
    write_activity_static(&dist_dir, &generated).unwrap();
}

fn should_build_activity(dist_dir: &Path) -> bool {
    if env::var_os("MAFIA_SKIP_ACTIVITY_BUILD").is_some() {
        return false;
    }
    env::var_os("MAFIA_FORCE_ACTIVITY_BUILD").is_some()
        || !dist_dir.join("index.html").is_file()
        || activity_source_changed(dist_dir)
}

fn build_activity(activity_dir: &Path, dist_dir: &Path) {
    if !activity_dir.join("package.json").is_file() {
        return;
    }

    if !activity_dir.join("node_modules").is_dir() {
        run_npm(activity_dir, &["ci"]);
    }
    run_npm(activity_dir, &["run", "build"]);

    if !dist_dir.join("index.html").is_file() {
        panic!("Activity UI build finished but activity/dist/index.html was not created.");
    }
}

fn run_npm(activity_dir: &Path, args: &[&str]) {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm)
        .args(args)
        .current_dir(activity_dir)
        .status()
        .unwrap_or_else(|err| panic!("failed to run `{npm} {}`: {err}", args.join(" ")));

    if !status.success() {
        panic!("`{npm} {}` failed with status {status}.", args.join(" "));
    }
}

fn activity_source_changed(dist_dir: &Path) -> bool {
    let Some(activity_dir) = dist_dir.parent() else {
        return false;
    };
    let Ok(dist_modified) = fs::metadata(dist_dir.join("index.html")).and_then(|m| m.modified())
    else {
        return true;
    };

    let mut newer = false;
    for path in [
        activity_dir.join("package.json"),
        activity_dir.join("package-lock.json"),
        activity_dir.join("index.html"),
        activity_dir.join("src"),
    ] {
        if path_newer_than(&path, dist_modified) {
            newer = true;
            break;
        }
    }
    newer
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

fn write_activity_static(dist_dir: &Path, generated: &Path) -> io::Result<()> {
    let mut entries = Vec::new();
    collect_files(dist_dir, dist_dir, &mut entries)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut file = fs::File::create(generated)?;
    writeln!(
        file,
        "pub struct EmbeddedActivityAsset {{ pub path: &'static str, pub content_type: &'static str, pub body: &'static [u8] }}"
    )?;
    writeln!(
        file,
        "pub static ACTIVITY_ASSETS: &[EmbeddedActivityAsset] = &["
    )?;
    for (url_path, fs_path) in entries {
        writeln!(
            file,
            "    EmbeddedActivityAsset {{ path: {:?}, content_type: {:?}, body: include_bytes!({:?}) }},",
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
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}
