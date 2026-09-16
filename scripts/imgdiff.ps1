#requires -Version 7
# Pixel diff between two screenshots: count and bounding box of differing pixels.
param([Parameter(Mandatory)][string]$A, [Parameter(Mandatory)][string]$B, [int]$Tolerance = 8)
Add-Type -AssemblyName System.Drawing
$imgA = [System.Drawing.Bitmap]::FromFile((Resolve-Path $A).Path)
$imgB = [System.Drawing.Bitmap]::FromFile((Resolve-Path $B).Path)
if ($imgA.Width -ne $imgB.Width -or $imgA.Height -ne $imgB.Height) {
  Write-Host "size differs: $($imgA.Width)x$($imgA.Height) vs $($imgB.Width)x$($imgB.Height)"
  exit 1
}
$minX = [int]::MaxValue; $minY = [int]::MaxValue; $maxX = -1; $maxY = -1; $n = 0
# Lock bits for speed.
$rect = New-Object System.Drawing.Rectangle(0, 0, $imgA.Width, $imgA.Height)
$da = $imgA.LockBits($rect, 'ReadOnly', 'Format32bppArgb')
$db = $imgB.LockBits($rect, 'ReadOnly', 'Format32bppArgb')
$len = [Math]::Abs($da.Stride) * $imgA.Height
$ba = New-Object byte[] $len; $bb = New-Object byte[] $len
[System.Runtime.InteropServices.Marshal]::Copy($da.Scan0, $ba, 0, $len)
[System.Runtime.InteropServices.Marshal]::Copy($db.Scan0, $bb, 0, $len)
$stride = [Math]::Abs($da.Stride)
for ($y = 0; $y -lt $imgA.Height; $y++) {
  $row = $y * $stride
  for ($x = 0; $x -lt $imgA.Width; $x++) {
    $i = $row + $x * 4
    $d = [Math]::Abs($ba[$i] - $bb[$i]) + [Math]::Abs($ba[$i + 1] - $bb[$i + 1]) + [Math]::Abs($ba[$i + 2] - $bb[$i + 2])
    if ($d -gt $Tolerance) {
      $n++
      if ($x -lt $minX) { $minX = $x }; if ($x -gt $maxX) { $maxX = $x }
      if ($y -lt $minY) { $minY = $y }; if ($y -gt $maxY) { $maxY = $y }
    }
  }
}
$imgA.UnlockBits($da); $imgB.UnlockBits($db)
$total = $imgA.Width * $imgA.Height
$imgA.Dispose(); $imgB.Dispose()
Write-Host ("differing pixels: {0} / {1} ({2:P2})" -f $n, $total, ($n / $total))
if ($n -gt 0) { Write-Host "bounding box: x=$minX..$maxX y=$minY..$maxY" }
