# R37: generate the legal format test corpus (every file made from FFmpeg's lavfi sources: smptehdbars, sine,
# testsrc2; nothing recorded, nothing downloaded) and check it through Milan Pro's engine. The corpus, the still image
# sequences, the HDR clips and the camera-card stand-ins go to -Out (default target\testmedia); nothing is committed.
# docs\FORMAT_SUPPORT.md's decode, capability and camera tables are rewritten from the results.
#
#   powershell -ExecutionPolicy Bypass -File tools\make-test-media.ps1 [-Out <folder>]
#
# Needs ffmpeg and ffprobe on PATH (or beside the build) and a Rust toolchain.
param([string]$Out = (Join-Path $PSScriptRoot '..\target\testmedia'))

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$env:MILAN_TESTMEDIA = (Resolve-Path $Out).Path
Push-Location (Join-Path $PSScriptRoot '..')
try {
    cargo test --release -- --ignored --test-threads=1 format_corpus_e2e image_sequence_e2e animated_gif_e2e hdr_view_transform_e2e camera_formats_e2e
    if ($LASTEXITCODE -ne 0) { throw "format tests failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}
