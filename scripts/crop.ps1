#requires -Version 7
# Crop a region of a screenshot and (optionally) magnify it, so small UI text is
# legible at native size after the image tool downscales large screenshots.
param(
  [Parameter(Mandatory)][string]$Image,
  [Parameter(Mandatory)][int]$X,
  [Parameter(Mandatory)][int]$Y,
  [Parameter(Mandatory)][int]$W,
  [Parameter(Mandatory)][int]$H,
  [Parameter(Mandatory)][string]$Out,
  [int]$Scale = 3
)
Add-Type -AssemblyName System.Drawing
$src = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Image).Path)
$W = [Math]::Min($W, $src.Width - $X); $H = [Math]::Min($H, $src.Height - $Y)
$dst = New-Object System.Drawing.Bitmap(($W * $Scale), ($H * $Scale))
$g = [System.Drawing.Graphics]::FromImage($dst)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
$g.DrawImage($src, (New-Object System.Drawing.Rectangle(0, 0, ($W * $Scale), ($H * $Scale))), (New-Object System.Drawing.Rectangle($X, $Y, $W, $H)), 'Pixel')
$g.Dispose()
$dst.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$src.Dispose(); $dst.Dispose()
Write-Host "cropped ${W}x${H} at $X,$Y -> $Out (x$Scale)"
