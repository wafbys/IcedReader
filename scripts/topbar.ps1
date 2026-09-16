#requires -Version 7
# Locate top-bar controls in a screenshot: groups columns that contain "ink"
# inside a horizontal band and prints each cluster's x-range, so clicks can
# target real coordinates instead of guessed ones. "Ink" = light label text on
# the dark chrome bar, or the orange fill of an active/primary button.
param(
  [Parameter(Mandatory)][string]$Image,
  [int]$Y0 = 28,
  [int]$Y1 = 60,
  [int]$Gap = 16
)
Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Image).Path)
$rect = New-Object System.Drawing.Rectangle(0, 0, $img.Width, $img.Height)
$d = $img.LockBits($rect, 'ReadOnly', 'Format32bppArgb')
$stride = [Math]::Abs($d.Stride)
$bytes = New-Object byte[] ($stride * $img.Height)
[System.Runtime.InteropServices.Marshal]::Copy($d.Scan0, $bytes, 0, $bytes.Length)
$img.UnlockBits($d)

$ink = New-Object bool[] $img.Width
for ($x = 0; $x -lt $img.Width; $x++) {
  for ($y = $Y0; $y -le $Y1; $y++) {
    $i = $y * $stride + $x * 4
    $b = $bytes[$i]; $g = $bytes[$i + 1]; $r = $bytes[$i + 2]
    # The chrome bar is dark (#2b2b2b-ish) with light labels; the primary and
    # active buttons are orange-filled. Either counts as "ink".
    $light = ([Math]::Min($r, [Math]::Min($g, $b)) -gt 135)
    $orange = ($r -gt 120 -and ($r - $b) -gt 40 -and ($r - $g) -gt 25)
    if ($light -or $orange) { $ink[$x] = $true; break }
  }
}

$runs = @()
$start = -1
$lastInk = -1
for ($x = 0; $x -lt $img.Width; $x++) {
  if ($ink[$x]) {
    if ($start -lt 0) { $start = $x }
    $lastInk = $x
  } elseif ($start -ge 0 -and ($x - $lastInk) -gt $Gap) {
    $runs += , @($start, $lastInk)
    $start = -1
  }
}
if ($start -ge 0) { $runs += , @($start, $lastInk) }

Write-Host "image $($img.Width)x$($img.Height), band y=$Y0..$Y1, gap=$Gap → $($runs.Count) clusters"
$n = 0
foreach ($r in $runs) {
  $n++
  Write-Host ("  #{0,-3} x={1,5}..{2,-5} w={3,-4} center={4}" -f $n, $r[0], $r[1], ($r[1] - $r[0] + 1), [int](($r[0] + $r[1]) / 2))
}
$img.Dispose()
