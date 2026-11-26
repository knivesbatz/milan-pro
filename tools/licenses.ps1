<#
.SYNOPSIS
    R56: writes docs/LICENSES.md and THIRD-PARTY-NOTICES.txt from Cargo.lock and the local Cargo registry.
.DESCRIPTION
    Reads every [[package]] of Cargo.lock. For each crates.io crate it opens
    %USERPROFILE%\.cargo\registry\src\index.crates.io-*\<name>-<version>\Cargo.toml (the copy `cargo build` already
    fetched; nothing is downloaded) and takes its `license` (or `license-file`) field and the names of its LICENSE*
    files. The output is deterministic (sorted, no dates), so re-running it on the same Cargo.lock and registry
    reproduces the committed files byte for byte. The hand-written part (FFmpeg, codecs, SDKs, fonts, installer) is
    tools/licenses-notes.md, appended as is.
.PARAMETER Check
    Regenerate into -CheckDir and compare with the committed docs/LICENSES.md and THIRD-PARTY-NOTICES.txt; exit 1 when
    they differ (the acceptance check; CI can run it). Nothing in the repository is written.
.PARAMETER CheckDir
    Where -Check writes its copies (default %TEMP%\milan-licenses-check).
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\licenses.ps1
    powershell -ExecutionPolicy Bypass -File tools\licenses.ps1 -Check
#>
param(
    [switch]$Check,
    [string]$CheckDir = (Join-Path $env:TEMP 'milan-licenses-check'),
    [string]$Lock = (Join-Path (Split-Path $PSScriptRoot -Parent) 'Cargo.lock'),
    [string]$Notes = (Join-Path $PSScriptRoot 'licenses-notes.md')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$mdOut = Join-Path $root 'docs\LICENSES.md'
$txtOut = Join-Path $root 'THIRD-PARTY-NOTICES.txt'
if ($Check) {
    $tmp = $CheckDir
    New-Item -ItemType Directory -Force $tmp | Out-Null
    $mdTarget = Join-Path $tmp 'LICENSES.md'
    $txtTarget = Join-Path $tmp 'THIRD-PARTY-NOTICES.txt'
} else {
    $mdTarget = $mdOut
    $txtTarget = $txtOut
}

# ---- Cargo.lock ----
$packages = @()
$cur = $null
foreach ($line in [IO.File]::ReadAllLines($Lock)) {
    if ($line -eq '[[package]]') {
        if ($cur) { $packages += $cur }
        $cur = [ordered]@{ name = ''; version = ''; source = '' }
    } elseif ($cur -and $line -match '^(name|version|source) = "(.*)"$') {
        $cur[$Matches[1]] = $Matches[2]
    }
}
if ($cur) { $packages += $cur }

# ---- Registry ----
$registry = Get-ChildItem (Join-Path $env:USERPROFILE '.cargo\registry\src') -Directory -Filter 'index.crates.io-*' | ForEach-Object { $_.FullName }
function Get-CrateDir($name, $version) {
    foreach ($r in $registry) {
        $d = Join-Path $r "$name-$version"
        if (Test-Path (Join-Path $d 'Cargo.toml')) { return $d }
    }
    return $null
}
function Read-Licence($dir) {
    # The `license` field of [package]; `license-file` when there is none. A multi-line Cargo.toml never puts these
    # outside [package], and the registry copy is normalised by cargo, so a line match is enough.
    $lic = ''
    $file = ''
    foreach ($l in [IO.File]::ReadAllLines((Join-Path $dir 'Cargo.toml'))) {
        if ($l -match '^license\s*=\s*"(.*)"') { $lic = $Matches[1] }
        elseif ($l -match '^license-file\s*=\s*"(.*)"') { $file = $Matches[1] }
        elseif ($l -match '^\[(dependencies|features|dev-dependencies|build-dependencies|target\.)') { break }
    }
    if (-not $lic -and $file) { $lic = "see $file" }
    return $lic
}

# The permissive expressions this project accepts (SPDX ids as crates write them; "/" and "OR" both mean a choice).
$allowed = @('MIT', 'Apache-2.0', 'Apache-2.0 WITH LLVM-exception', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', 'Unicode-3.0', 'Unicode-DFS-2016', 'MPL-2.0', 'CC0-1.0', 'BSL-1.0', 'OFL-1.1', 'Ubuntu-font-1.0', 'CDLA-Permissive-2.0', '0BSD', 'Unlicense', 'OpenSSL')
function Test-Permissive($expr) {
    # True when at least one alternative of every AND-group is in the allowed list.
    if (-not $expr) { return $false }
    foreach ($andPart in ($expr -split '\bAND\b')) {
        $ok = $false
        foreach ($alt in ($andPart -replace '[()]', '' -split '(\bOR\b|/)')) {
            $a = $alt.Trim()
            if ($a -eq '' -or $a -eq 'OR' -or $a -eq '/') { continue }
            if ($allowed -contains $a) { $ok = $true }
        }
        if (-not $ok) { return $false }
    }
    return $true
}

$rows = @()
foreach ($p in ($packages | Sort-Object { $_.name }, { $_.version })) {
    $row = [ordered]@{ name = $p.name; version = $p.version; licence = ''; files = ''; kind = ''; flag = ''; dir = '' }
    if ($p.name -eq 'milan-pro' -and -not $p.source) {
        $row.kind = 'this project'
        $row.licence = 'MIT'
        $row.files = 'LICENSE'
    } elseif ($p.source -like 'registry+*') {
        $row.kind = 'crates.io'
        $dir = Get-CrateDir $p.name $p.version
        if ($dir) {
            $row.dir = $dir
            $row.licence = Read-Licence $dir
            $row.files = (Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE)' } | ForEach-Object { $_.Name } | Sort-Object) -join ', '
            if (-not $row.licence) { $row.flag = 'no licence field' }
            elseif (-not (Test-Permissive $row.licence)) { $row.flag = 'not in the permissive list' }
        } else {
            $row.licence = '(not in the local registry)'
            $row.flag = 'not fetched locally: run cargo build first'
        }
    } elseif ($p.source -like 'git+*') {
        $row.kind = 'git: ' + $p.source
        $row.licence = '(git dependency: read its repository)'
        $row.flag = 'git dependency'
    } elseif (-not $p.source) {
        $row.kind = 'path'
        $row.licence = '(path dependency in this repository: MIT)'
    } else {
        $row.kind = $p.source
        $row.flag = 'unknown source'
    }
    $rows += [pscustomobject]$row
}

# ---- Summary ----
$counts = @{}
foreach ($r in $rows) {
    $k = $r.licence
    if (-not $k) { $k = '(none)' }
    if ($counts.ContainsKey($k)) { $counts[$k] += 1 } else { $counts[$k] = 1 }
}
$flagged = $rows | Where-Object { $_.flag }
# A copyleft term in an expression: a problem when it is the only choice, a footnote when MIT/Apache is offered beside it.
$copyleftAny = $rows | Where-Object { $_.licence -match '\b(A?GPL|LGPL|SSPL|EUPL|CC-BY-SA)' }
$copyleft = @($copyleftAny | Where-Object { -not (Test-Permissive $_.licence) })
$copyleftAlt = @($copyleftAny | Where-Object { Test-Permissive $_.licence })

# ---- Markdown ----
$md = New-Object System.Text.StringBuilder
function L($s = '') { [void]$md.Append($s).Append("`n") }
L '# Milan Pro licences'
L ''
L 'Generated by `tools/licenses.ps1` from `Cargo.lock` and the local Cargo registry (the crates `cargo build` fetched; the script downloads nothing). Do not edit the tables by hand: run the script again and commit the result. `tools/licenses.ps1 -Check` regenerates into a temporary folder and fails when the committed files are stale; that is the acceptance check (cargo-deny is not required, see the notes below). The hand-written notes after the tables live in `tools/licenses-notes.md`.'
L ''
L "Milan Pro itself is MIT (see [LICENSE](../LICENSE)). Crates in ``Cargo.lock``: $($packages.Count) (one is this project). The installer ships the same list as ``THIRD-PARTY-NOTICES.txt``."
L ''
L '## Summary by licence expression'
L ''
L '| Licence (as the crate states it) | Crates |'
L '|---|---|'
foreach ($k in ($counts.Keys | Sort-Object { -$counts[$_] }, { $_ })) { L "| $k | $($counts[$k]) |" }
L ''
L '## Flagged'
L ''
if ($copyleft.Count -eq 0) {
    L 'No crate in `Cargo.lock` carries a GPL, LGPL, AGPL, SSPL or other copyleft licence. (FFmpeg, which is GPL, is not a crate: see the notes.)'
} else {
    L 'Copyleft licences found (the notes say how each is handled):'
    L ''
    foreach ($r in $copyleft) { L "- ``$($r.name) $($r.version)``: $($r.licence)" }
}
if ($copyleftAlt.Count -gt 0) {
    L ''
    L 'Crates that offer a copyleft licence as one alternative beside MIT or Apache-2.0 (Milan Pro takes the permissive one; listed so nobody mistakes them for a problem):'
    L ''
    foreach ($r in $copyleftAlt) { L "- ``$($r.name) $($r.version)``: $($r.licence)" }
}
L ''
if ($flagged.Count -eq 0) {
    L 'Every crate states a licence from the permissive list the script accepts (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0 / Unicode-DFS-2016, MPL-2.0, CC0-1.0, BSL-1.0, OFL-1.1, Ubuntu-font-1.0, CDLA-Permissive-2.0, 0BSD, Unlicense), and every one was found in the local registry.'
} else {
    L 'Needing a look (not in the permissive list, no licence field, not found locally, git or unknown source):'
    L ''
    L '| Crate | Version | Licence | Why flagged |'
    L '|---|---|---|---|'
    foreach ($r in $flagged) { L "| $($r.name) | $($r.version) | $($r.licence) | $($r.flag) |" }
}
L ''
L '## Every crate'
L ''
L '| Crate | Version | Licence | Source | Licence files in the crate |'
L '|---|---|---|---|---|'
foreach ($r in $rows) { L "| $($r.name) | $($r.version) | $($r.licence) | $($r.kind) | $($r.files) |" }
L ''
$notesText = [IO.File]::ReadAllText($Notes) -replace "`r`n", "`n"
[void]$md.Append($notesText)
if (-not $notesText.EndsWith("`n")) { [void]$md.Append("`n") }

# ---- Plain text for the installer ----
$txt = New-Object System.Text.StringBuilder
function T($s = '') { [void]$txt.Append($s).Append("`n") }
T 'THIRD-PARTY NOTICES FOR MILAN PRO'
T '================================='
T ''
T 'Milan Pro is MIT-licensed (LICENSE.txt). It is built from the Rust crates listed below, each under its own'
T 'licence as stated by its authors, and it runs FFmpeg as a separate program (see the FFmpeg section). This file'
T 'is generated by tools/licenses.ps1 from Cargo.lock; docs/LICENSES.md in the source repository is the same'
T 'information with its tables.'
T ''
T 'CRATES'
T '------'
T ''
$w1 = ($rows | ForEach-Object { $_.name.Length } | Measure-Object -Maximum).Maximum
$w2 = ($rows | ForEach-Object { $_.version.Length } | Measure-Object -Maximum).Maximum
foreach ($r in $rows) { T ("{0,-$w1}  {1,-$w2}  {2}" -f $r.name, $r.version, $r.licence) }
T ''
T 'The licence texts follow the notes, under LICENCE TEXTS: each distinct text once, with the crates that carry it.'
T ''
T 'NOTES'
T '-----'
T ''
# The notes, with Markdown stripped to plain text: headings become upper-case lines, links keep their URL.
foreach ($l in ($notesText -split "`n")) {
    if ($l -eq '## Notes') { continue }  # the NOTES heading above stands for it
    $t = $l
    if ($t -match '^#+\s*(.*)$') { $t = $Matches[1].ToUpperInvariant() }
    $t = $t -replace '\[([^\]]+)\]\(([^)]+)\)', '$1 ($2)'
    $t = $t -replace '`', ''
    $t = $t -replace '\*\*', ''
    T $t
}

# ---- Licence texts (plain text only): the copyright and permission notices MIT, BSD, ISC and Zlib ask to be kept with
# binaries. For a choice of licences the MIT file is taken when MIT is offered, else the Apache file; NOTICE files are
# always kept. Identical texts (after line-ending and trailing-space normalisation) are printed once.
function Get-LicenceFiles($dir, $expr) {
    $all = @(Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)' })
    foreach ($sub in @('fonts', 'LICENSES', 'licenses')) {
        $d = Join-Path $dir $sub
        if (Test-Path $d) { $all += @(Get-ChildItem $d -File | Where-Object { $_.Name -match '(LICEN[CS]E|COPYING|OFL|UFL|\.txt$)' -and $_.Extension -ne '.ttf' }) }
    }
    $all = @($all | Sort-Object FullName)
    $notice = @($all | Where-Object { $_.Name -match '^NOTICE' })
    $main = @($all | Where-Object { $_.Name -notmatch '^NOTICE' })
    $alts = @(($expr -replace '[()]', '') -split '\s*(\bOR\b|/)\s*' | ForEach-Object { $_.Trim() })
    $pick = $main
    if ($expr -notmatch '\bAND\b' -and $main.Count -gt 1) {
        $mit = @($main | Where-Object { $_.Name -match 'MIT' })
        $apache = @($main | Where-Object { $_.Name -match 'APACHE' })
        if ($alts -contains 'MIT' -and $mit.Count -gt 0) { $pick = $mit }
        elseif (($alts -contains 'Apache-2.0') -and $apache.Count -gt 0) { $pick = $apache }
    }
    return @($pick) + @($notice)
}
$texts = [ordered]@{}
$noFile = @()
foreach ($r in $rows) {
    if (-not $r.dir) { continue }
    $files = @(Get-LicenceFiles $r.dir $r.licence)
    if ($files.Count -eq 0) { $noFile += $r; continue }
    foreach ($f in $files) {
        $body = (([IO.File]::ReadAllText($f.FullName) -replace "`r`n", "`n" -replace "`r", "`n") -split "`n" | ForEach-Object { $_.TrimEnd() }) -join "`n"
        $body = $body.Trim("`n")
        if (-not $texts.Contains($body)) { $texts[$body] = New-Object System.Collections.ArrayList }
        [void]$texts[$body].Add("$($r.name) $($r.version) ($($f.Name))")
    }
}
T ''
T 'LICENCE TEXTS'
T '-------------'
T ''
T 'Each text below is copied from the crate packages named above it, as found in the local Cargo registry.'
if ($noFile.Count -gt 0) {
    T ''
    T 'These crates carry no licence file in their package. Their licence is the one named in the CRATES list, in its'
    T 'standard SPDX text (https://spdx.org/licenses/<id>.html); the MIT and Apache-2.0 texts below match it apart'
    T 'from their copyright lines:'
    foreach ($r in $noFile) { T "  $($r.name) $($r.version): $($r.licence)" }
}
foreach ($k in $texts.Keys) {
    T ''
    T ('=' * 100)
    foreach ($who in $texts[$k]) { T $who }
    T ('-' * 100)
    T $k
}

$enc = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText($mdTarget, $md.ToString(), $enc)
[IO.File]::WriteAllText($txtTarget, $txt.ToString(), $enc)

if ($Check) {
    $bad = 0
    foreach ($pair in @(@($mdTarget, $mdOut), @($txtTarget, $txtOut))) {
        if (-not (Test-Path $pair[1])) { Write-Host "MISSING $($pair[1])"; $bad = 1; continue }
        $a = [IO.File]::ReadAllText($pair[0]) -replace "`r`n", "`n"
        $b = [IO.File]::ReadAllText($pair[1]) -replace "`r`n", "`n"
        if ($a -ne $b) { Write-Host "STALE $($pair[1]) (regenerated copy: $($pair[0]))"; $bad = 1 }
        else { Write-Host "OK $($pair[1])" }
    }
    exit $bad
}
Write-Host "Wrote $mdTarget and $txtTarget ($($rows.Count) crates, $($flagged.Count) flagged, $($copyleft.Count) copyleft, $($copyleftAlt.Count) with a copyleft alternative)"
