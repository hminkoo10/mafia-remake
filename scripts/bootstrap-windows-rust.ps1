$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$CargoHome = Join-Path $RepoRoot ".cargo"
$RustupHome = Join-Path $RepoRoot ".rustup"
$RustupInit = Join-Path $RepoRoot ".rustup-init.exe"
$MingwLib = Join-Path $RepoRoot ".mingw\lib"

New-Item -ItemType Directory -Force -Path $CargoHome, $RustupHome | Out-Null

if (!(Test-Path (Join-Path $CargoHome "bin\rustup.exe"))) {
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $RustupInit
}

$env:CARGO_HOME = $CargoHome
$env:RUSTUP_HOME = $RustupHome
$env:Path = "$CargoHome\bin;$env:Path"

if (!(Test-Path (Join-Path $CargoHome "bin\rustup.exe"))) {
    & $RustupInit -y --profile minimal --default-toolchain stable --no-modify-path
}

rustup target add x86_64-pc-windows-gnullvm
rustup component add clippy rustfmt llvm-tools

New-Item -ItemType Directory -Force -Path $MingwLib | Out-Null
$MsysLib = "C:\msys64\ucrt64\lib"
if (Test-Path $MsysLib) {
    Copy-Item -Path (Join-Path $MsysLib "*.a") -Destination $MingwLib -Force
    Copy-Item -Path (Join-Path $MsysLib "*.o") -Destination $MingwLib -Force
} else {
    Write-Warning "C:\msys64\ucrt64\lib not found. Install MSYS2 UCRT64 or copy MinGW import libraries into .mingw\lib."
}

$GccRoot = "C:\msys64\ucrt64\lib\gcc\x86_64-w64-mingw32"
$GccLib = Get-ChildItem -Path $GccRoot -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1
if ($GccLib) {
    Copy-Item -Path (Join-Path $GccLib.FullName "libgcc*.a") -Destination $MingwLib -Force
    Copy-Item -Path (Join-Path $GccLib.FullName "crt*.o") -Destination $MingwLib -Force
    $GccEh = Join-Path $GccLib.FullName "libgcc_eh.a"
    if (Test-Path $GccEh) {
        Copy-Item -LiteralPath $GccEh -Destination (Join-Path $MingwLib "libunwind.a") -Force
    }
} else {
    $GnuLlvmLib = Join-Path $RustupHome "toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnullvm\lib"
    $Unwind = Get-ChildItem -Path $GnuLlvmLib -Filter "libunwind-*.rlib" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($Unwind) {
        Copy-Item -LiteralPath $Unwind.FullName -Destination (Join-Path $MingwLib "libunwind.a") -Force
    }
}

rustc -V
cargo -V
