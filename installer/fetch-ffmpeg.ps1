# Downloads the pinned FFmpeg build bundled by the installer into installer\ffmpeg.
$ErrorActionPreference = 'Stop'

$Version = '9.0.2'
$Name    = "ffmpeg-$Version-full_build-shared"
$Url     = "https://github.com/GyanD/codexffmpeg/releases/download/$Version/$Name.zip"
$Sha256  = '8d31e162f1616e37aab3fa2db991b97e1b8dbeb1c8465fd81a81be16ff91b328'

$Out = Join-Path $PSScriptRoot 'ffmpeg'
if (Test-Path (Join-Path $Out "ffmpeg.exe")) { Write-Host "FFmpeg already present in $Out"; exit 0 }

$Tmp = Join-Path ([IO.Path]::GetTempPath()) "milanpro-ffmpeg-$Version"
New-Item -ItemType Directory -Force $Tmp | Out-Null
$Zip = Join-Path $Tmp "$Name.zip"

if (-not (Test-Path $Zip) -or (Get-FileHash $Zip -Algorithm SHA256).Hash -ne $Sha256) {
    Write-Host "Downloading $Url"
    Invoke-WebRequest $Url -OutFile $Zip -UseBasicParsing
}
$Hash = (Get-FileHash $Zip -Algorithm SHA256).Hash
if ($Hash -ne $Sha256) { throw "FFmpeg checksum mismatch: got $Hash, expected $Sha256" }

$Extract = Join-Path $Tmp 'x'
if (Test-Path $Extract) { Remove-Item -Recurse -Force $Extract }
Expand-Archive $Zip -DestinationPath $Extract
$Root = Join-Path $Extract $Name

New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item (Join-Path $Root 'bin\ffmpeg.exe'), (Join-Path $Root 'bin\ffprobe.exe') $Out
Copy-Item (Join-Path $Root 'bin\*.dll') $Out
Copy-Item (Join-Path $Root 'LICENSE') (Join-Path $Out 'FFMPEG-LICENSE.txt')
Copy-Item (Join-Path $Root 'README.txt') (Join-Path $Out 'FFMPEG-README.txt')
@"
Milan Pro bundles FFmpeg $Version (Windows build by Gyan Doshi, $Name),
which is licensed under the GNU GPL v3. FFmpeg runs as a separate program.

FFmpeg source:  https://ffmpeg.org/releases/ffmpeg-$Version.tar.xz
Build source:   https://github.com/GyanD/codexffmpeg/releases/tag/$Version
Build details:  see FFMPEG-README.txt
"@ | Set-Content (Join-Path $Out 'FFMPEG-SOURCE.txt') -Encoding utf8

Remove-Item -Recurse -Force $Extract
Write-Host "FFmpeg $Version ready in $Out"
