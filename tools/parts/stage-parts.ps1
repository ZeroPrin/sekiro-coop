# Optional game-asset package for a release build (FORK.md "Parts package").
#
#   stage-parts.ps1 -Stage <release folder>   copy the parts into <Stage>\mod\parts\ and add the
#                                             [[packages]] block to <Stage>\sekiro-coop.me3
#   stage-parts.ps1 -Check <file.dcx>...      only run the Kraken reset check
#
# Sources: $env:SEKIRO_COOP_PARTS, a ';'-separated list of folders that hold *.dcx part files
# (for example E:\RE\work_override\fixed\parts;E:\RE\work_skin\game\mod\parts). Unset or empty:
# nothing is staged and the .me3 is left as shipped (exit 0). The assets are game files and
# never go into git (.gitignore).
#
# Every DCX_KRAK file must carry the decoder-reset flag on every 256 KiB Oodle block. The
# game's loader decodes the blocks of a part file independently; a stream compressed without
# seekChunkReset stalled the loader live (the first wardrobe part files, fixed 2026-11-14). Any
# file that fails, or any file that is not a .dcx, stops the build.
[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Stage,
    [switch]$Check,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Files
)
$ErrorActionPreference = 'Stop'

function Read-BE32([byte[]]$d, [int]$o) {
    return ([uint32]$d[$o] -shl 24) -bor ([uint32]$d[$o + 1] -shl 16) -bor ([uint32]$d[$o + 2] -shl 8) -bor [uint32]$d[$o + 3]
}

# Returns $null when the file is fine, else the reason. Port of the block walk in
# E:\RE\work_override\tools\anal.py (blocks) / scanreset.py: each 256 KiB of output starts with
# a 2-byte block header (b0 low nibble 0xC, bit 7 = decoder reset, bit 6 = uncompressed), then
# for a compressed block a 3-byte quantum header (6 with a CRC; 0x3FFFF = memset, 4 bytes).
function Test-KrakenResets([string]$path) {
    $d = [IO.File]::ReadAllBytes($path)
    if ($d.Length -lt 0x4C -or [Text.Encoding]::ASCII.GetString($d, 0, 4) -ne "DCX`0") { return 'not a DCX file' }
    $fmt = [Text.Encoding]::ASCII.GetString($d, 0x28, 4)
    if ($fmt -eq 'DFLT') { return $null }
    if ($fmt -ne 'KRAK') { return "unknown DCX format '$fmt'" }
    $usize = [int64](Read-BE32 $d 0x1C)
    $csize = [int64](Read-BE32 $d 0x20)
    $off = [int64](Read-BE32 $d 0x14)
    if ($off + $csize -gt $d.Length) { return "stream ($off + $csize) runs past the file end ($($d.Length))" }
    $p = $off; $end = $off + $csize; $out = [int64]0; $n = 0; $reset = 0
    while ($out -lt $usize) {
        if ($p + 2 -gt $end) { return "stream ends inside block $n" }
        $b0 = $d[$p]; $b1 = $d[$p + 1]
        if (($b0 -band 0xF) -ne 0xC) { return ('block {0}: bad header byte 0x{1:X2} at stream offset {2}' -f $n, $b0, ($p - $off)) }
        if ($b0 -band 0x80) { $reset++ }
        $p += 2
        $left = [Math]::Min(0x40000, $usize - $out)
        if ($b0 -band 0x40) {
            $p += $left
        } else {
            if ($p + 3 -gt $end) { return "stream ends inside block $n" }
            $v = ([int]$d[$p] -shl 16) -bor ([int]$d[$p + 1] -shl 8) -bor [int]$d[$p + 2]
            $size = $v -band 0x3FFFF
            if ($size -ne 0x3FFFF) {
                if ($b1 -band 0x80) { $p += 6 } else { $p += 3 }
                $p += $size + 1
            } else {
                $p += 4
            }
        }
        if ($p -gt $end) { return "block $n runs past the stream end" }
        $out += $left; $n++
    }
    if ($reset -ne $n) { return "only $reset of $n Kraken blocks carry a decoder reset (compressed without seekChunkReset)" }
    return $null
}

function Assert-Dcx([string[]]$paths) {
    $bad = 0
    foreach ($f in $paths) {
        $why = Test-KrakenResets $f
        if ($why) { Write-Host "parts: REFUSED $f : $why"; $bad++ } else { Write-Host "parts: ok $f" }
    }
    if ($bad) { throw "$bad part file(s) failed the Kraken reset check" }
}

if ($Check) {
    if (-not $Files) { throw 'usage: stage-parts.ps1 -Check <file.dcx>...' }
    Assert-Dcx $Files; exit 0
}
if (-not $Stage) { throw 'usage: stage-parts.ps1 -Stage <folder> | -Check <file.dcx>...' }

$sources = @("$env:SEKIRO_COOP_PARTS".Split(';') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
if ($sources.Count -eq 0) { Write-Host 'parts: SEKIRO_COOP_PARTS not set, no parts package'; exit 0 }

$parts = @{}
foreach ($src in $sources) {
    if (-not (Test-Path -LiteralPath $src -PathType Container)) { throw "parts: source folder $src does not exist" }
    foreach ($f in Get-ChildItem -LiteralPath $src -File) {
        if ($f.Extension -ne '.dcx') { throw "parts: $($f.FullName) is not a .dcx file" }
        $key = $f.Name.ToLowerInvariant()
        if ($parts.ContainsKey($key)) { throw "parts: $($f.Name) is in two sources ($($parts[$key]) and $($f.FullName))" }
        $parts[$key] = $f.FullName
    }
}
if ($parts.Count -eq 0) { throw "parts: no .dcx files in $($sources -join ';')" }
Assert-Dcx @($parts.Values | Sort-Object)

$me3 = Join-Path $Stage 'sekiro-coop.me3'
$text = [IO.File]::ReadAllText($me3)
if ($text -match '\[\[packages\]\]') { throw "parts: $me3 already has a [[packages]] block" }
$dst = Join-Path $Stage 'mod\parts'
New-Item -ItemType Directory -Force $dst | Out-Null
foreach ($f in $parts.Values | Sort-Object) { Copy-Item -LiteralPath $f -Destination $dst }
# me3 resolves `path` relative to the .me3; the folder mirrors the game's virtual root.
$nl = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }
$text = $text.TrimEnd() + "$nl$nl[[packages]]${nl}id = `"sekiro-together-parts`"${nl}path = 'mod'$nl"
[IO.File]::WriteAllText($me3, $text, (New-Object Text.UTF8Encoding $false))
Write-Host "parts: $($parts.Count) files staged into $dst, [[packages]] block written"
