# Run the image-to-video model locally. No API, account, or plugin is used.
param(
    [Parameter(Mandatory)][string]$RuntimeRoot,
    # return: deal/flip의 마지막 프레임에서 이어 손을 슈 위 제자리로 돌린다 (-Reference로 그 프레임을 준다).
    [ValidateSet('idle', 'deal', 'flip', 'return')][string]$Mood = 'deal',
    [string]$OutputDirectory = '',
    [int]$Width = 768,
    [int]$Height = 512,
    [int]$Frames = 73,
    [int]$Seed = 473,
    [ValidateSet('dmd', 'euler')][string]$Sampling = 'euler',
    [ValidateSet('full', 'tiny')][string]$Decoder = 'full',
    # Start frame. Use a frame the model already generated so every clip keeps the same face.
    [string]$Reference = '',
    [string]$Suffix = ''
)
$ErrorActionPreference = 'Stop'
if ($Width -lt 64 -or $Height -lt 64 -or $Frames -lt 5 -or $Width % 32 -ne 0 -or $Height % 32 -ne 0 -or $Frames % 4 -ne 1) {
    throw 'Width/height must be at least 64 and multiples of 32; frames must be 4n+1 and at least 5.'
}
$RuntimeRoot = [IO.Path]::GetFullPath($RuntimeRoot)
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $RuntimeRoot 'output' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if ($RuntimeRoot -match '[^\x00-\x7F]' -or $OutputDirectory -match '[^\x00-\x7F]') {
    throw 'Use ASCII runtime/output paths for the native image loader, for example C:/temp/casino-local-video.'
}
$taskReference = if ($Reference) { [IO.Path]::GetFullPath($Reference) } else { Join-Path $PSScriptRoot 'reference/sophia-table.png' }
$taskCli = Join-Path $RuntimeRoot 'runtime/sd-cli.exe'
$taskModel = Join-Path $RuntimeRoot 'models/FastWan2.2-TI2V-5B-q6_k.gguf'
$taskText = Join-Path $RuntimeRoot 'models/umt5-xxl-encoder-Q4_K_M.gguf'
$taskVae = Join-Path $RuntimeRoot $(if ($Decoder -eq 'tiny') { 'models/taew2_2.safetensors' } else { 'models/wan2.2_vae.safetensors' })
$taskVaeFlag = if ($Decoder -eq 'tiny') { '--tae' } else { '--vae' }
foreach ($taskFile in @($taskCli, $taskModel, $taskText, $taskVae, $taskReference)) {
    if (-not (Test-Path -LiteralPath $taskFile -PathType Leaf)) { throw "Missing file: $taskFile" }
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$taskOutput = Join-Path $OutputDirectory "sophia-$Mood$Suffix.webm"
if (Test-Path -LiteralPath $taskOutput) { throw "Output already exists: $taskOutput" }

$taskScene = 'Locked-off tripod camera. One woman remains seated, same pose and face as the input photograph. A real rectangular wooden playing-card dispenser holds a visible stack of small white cards on the right side of the green table. Both of her bare forearms stay visible, attached to her small natural hands. Her head, hair, face, dress and the background stay still and unchanged. Soft diffuse lighting, balanced photographic exposure, detailed skin texture, neutral natural colors. Only a subtle hand gesture happens. '
$taskMotion = @{
    idle = 'She breathes gently and blinks once, looking calmly at the camera. Both hands remain resting on the green felt and the rectangular playing-card dispenser. Her head stays upright, mouth closed. Barely perceptible natural motion, ending in the same resting position.'
    deal = 'Using her own hand at image right, she pulls one small flat white playing card from the rectangular card dispenser. She smoothly slides this single playing card onto the green felt in front of herself, keeping her elbow close to her body. The other hand rests flat on the felt. She retracts her hand to its original position.'
    flip = 'Her own hand resting near the middle of the green felt makes a small controlled wrist turn, revealing the front of a single small rectangular playing card on the table. Then she rests her hand flat again. Her other hand remains on the rectangular card dispenser. Her head stays upright and still.'
    return = 'She leaves the playing card on the felt and slowly slides her right hand back along the felt until it rests on top of the rectangular card dispenser at image right. Her left hand stays flat on the green felt. Her head, face, shoulders and body stay still and upright, calm neutral expression. Slow, small, natural hand motion only.'
}
$taskInput = Join-Path $RuntimeRoot ('reference-' + [guid]::NewGuid().ToString('N') + '.png')
Copy-Item -LiteralPath $taskReference -Destination $taskInput
$taskArguments = @(
    '-M', 'vid_gen', '--diffusion-model', $taskModel, '--t5xxl', $taskText, $taskVaeFlag, $taskVae,
    '--backend', 'vulkan0', '--params-backend', 'disk', '--mmap', '--max-vram', '3',
    '--diffusion-fa', '--vae-tiling', '--temporal-tiling', '--vae-tile-size', '8x8', '--vae-tile-overlap', '0.125',
    '-t', '8', '-W', "$Width", '-H', "$Height", '--video-frames', "$Frames", '--fps', '24',
    '--steps', '3', '--cfg-scale', '1',
    '--flow-shift', '3', '-s', "$Seed", '-i', $taskInput, '-p', ($taskScene + $taskMotion[$Mood]), '-o', $taskOutput
)
if ($Sampling -eq 'euler') { $taskArguments += @('--sampling-method', 'euler', '--scheduler', 'lcm') }
else { $taskArguments += @('--sampling-method', 'lcm', '--sigmas', '1,0.757,0.522,0') }
try {
    # Windows PowerShell turns native progress on stderr into errors; the exit code decides failure.
    $ErrorActionPreference = 'Continue'
    & $taskCli @taskArguments
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) { throw "Generation failed: exit $LASTEXITCODE" }
    if (-not (Test-Path -LiteralPath $taskOutput -PathType Leaf)) { throw 'Generator returned without a video.' }
} finally {
    Remove-Item -LiteralPath $taskInput -ErrorAction SilentlyContinue
}
[ordered]@{
    model = [IO.Path]::GetFileName($taskModel)
    decoder = [IO.Path]::GetFileName($taskVae)
    reference = [IO.Path]::GetFileName($taskReference)
    width = $Width; height = $Height; frames = $Frames; fps = 24; seed = $Seed
    sampling = $Sampling
    prompt = $taskScene + $taskMotion[$Mood]
    generated_at = [DateTime]::UtcNow.ToString('o')
    reviewed = $false
} | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath ($taskOutput + '.json') -Encoding utf8
Write-Output "Generated locally: $taskOutput. Review hands, face, cards and loop before copying into public/dealers."
