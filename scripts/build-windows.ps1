$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$CargoHome = Join-Path $RepoRoot ".cargo"
$RustupHome = Join-Path $RepoRoot ".rustup"

if (!(Test-Path (Join-Path $CargoHome "bin\cargo.exe"))) {
    throw "Repo-local Rust missing. Run scripts\bootstrap-windows-rust.ps1 first."
}

if (!(Test-Path (Join-Path $RepoRoot ".mingw\lib\libkernel32.a"))) {
    throw "Repo-local MinGW libraries missing. Run scripts\bootstrap-windows-rust.ps1 first."
}

$env:CARGO_HOME = $CargoHome
$env:RUSTUP_HOME = $RustupHome
$env:Path = "$CargoHome\bin;$env:Path"

Push-Location $RepoRoot
try {
    cargo build --release --target x86_64-pc-windows-gnullvm
} finally {
    Pop-Location
}
