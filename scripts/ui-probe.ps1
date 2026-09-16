#requires -Version 7
<#
UI probe for IcedReader: launches the portable exe, drives it with real
keyboard/mouse input and captures the window client area to PNG. A verification
tool, not part of the product build — pair it with crop.ps1 / imgdiff.ps1 /
topbar.ps1 when a change needs to be checked against a real window.

  pwsh -File scripts/ui-probe.ps1 -Exe target/release/IcedReader.exe `
       -Book "fixtures/sample.epub" -OutDir target/ui-shots `
       -Steps 'sleep:2500;shot:01-open;key:{RIGHT};sleep:600;shot:02-next'

Steps are separated by ';' (a comma-joined array does not survive `pwsh -File`).

Step grammar:
  sleep:MS              wait
  shot:NAME             capture client area to <OutDir>/NAME.png
  key:KEYS              SendKeys — named keys need braces: {RIGHT} {PGDN}
                        {HOME} {END} {ESC} {F11}, modifiers ^a %{F4}
  type:TEXT             SendKeys literal text
  move:X,Y              move the cursor (client coordinates)
  click:X,Y             click (client coordinates)
  dblclick:X,Y          double click
  wheel:N               wheel at the current cursor position (N<0 = down)
  resize:WxH            move/resize the window
  maximize / restore    window state
  dump:LABEL            print the client size and cursor position
  mem:LABEL             print app + WebView2 tree working set
  close                 graceful close (CloseMainWindow) and wait for exit

Coordinates are **physical client pixels** (the probe is DPI aware); on a 150%
display, CSS pixels × 1.5. Buttons move with their labels — take a shot and
measure with topbar.ps1 instead of guessing.
#>
param(
  [Parameter(Mandatory)][string]$Exe,
  [string]$Book,
  [Parameter(Mandatory)][string]$OutDir,
  [string]$Steps = 'sleep:2500;shot:01-open',
  [int]$Width = 1120,
  [int]$Height = 780,
  [switch]$KeepOpen
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, int data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int t, bool repaint);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SetFocus(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  public static readonly IntPtr HWND_TOPMOST = new IntPtr(-1);
  public static readonly IntPtr HWND_NOTOPMOST = new IntPtr(-2);
  public const uint SWP_NOMOVE = 0x0002, SWP_NOSIZE = 0x0001, SWP_SHOWWINDOW = 0x0040;
  public const uint MOUSEEVENTF_WHEEL = 0x0800;
  public const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
  public const uint MOUSEEVENTF_LEFTUP = 0x0004;

  // Windows refuses SetForegroundWindow from a process that does not own the
  // foreground; attaching our input queue to the current foreground thread is
  // the supported workaround.
  public static void ForceForeground(IntPtr h) {
    if (IsIconic(h)) ShowWindow(h, 9);
    SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
    IntPtr fg = GetForegroundWindow();
    uint fgThread = fg == IntPtr.Zero ? 0 : GetWindowThreadProcessId(fg, IntPtr.Zero);
    uint myThread = GetCurrentThreadId();
    bool attached = fgThread != 0 && fgThread != myThread && AttachThreadInput(myThread, fgThread, true);
    BringWindowToTop(h);
    SetForegroundWindow(h);
    SetFocus(h);
    if (attached) AttachThreadInput(myThread, fgThread, false);
  }
  public static void DropTopmost(IntPtr h) {
    SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
  }
  public static bool IsForeground(IntPtr h) { return GetForegroundWindow() == h; }
}
'@

[void][W]::SetProcessDPIAware()

$Exe = (Resolve-Path $Exe).Path
if ($Book) { $Book = (Resolve-Path $Book).Path }
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

function Get-ClientOrigin([IntPtr]$h) {
  $p = New-Object W+POINT
  $p.X = 0; $p.Y = 0
  [void][W]::ClientToScreen($h, [ref]$p)
  return $p
}

function Get-ClientSize([IntPtr]$h) {
  $r = New-Object W+RECT
  [void][W]::GetClientRect($h, [ref]$r)
  return @{ W = $r.Right - $r.Left; H = $r.Bottom - $r.Top }
}

function Save-Shot([string]$name) {
  $h = $script:Hwnd
  $sz = Get-ClientSize $h
  $org = Get-ClientOrigin $h
  if ($sz.W -le 0 -or $sz.H -le 0) { Write-Host "  ! shot $name skipped: empty client"; return }
  $bmp = New-Object System.Drawing.Bitmap($sz.W, $sz.H)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($org.X, $org.Y, 0, 0, (New-Object System.Drawing.Size($sz.W, $sz.H)))
  $g.Dispose()
  $path = Join-Path $OutDir "$name.png"
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Host "  shot $name  ($($sz.W)x$($sz.H) at $($org.X),$($org.Y)) -> $path"
}

function Focus-App {
  $h = $script:Hwnd
  [W]::ForceForeground($h)
  Start-Sleep -Milliseconds 300
  if (-not [W]::IsForeground($h)) {
    # Second attempt is usually enough once our queue is attached.
    [W]::ForceForeground($h)
    Start-Sleep -Milliseconds 400
  }
  if (-not [W]::IsForeground($h)) { Write-Host "  ! window is NOT foreground; capture/keys may hit another app" }
}

# ---- launch -------------------------------------------------------------
$env:ICED_READER_OPEN = if ($Book) { $Book } else { $null }
if (-not $Book) { Remove-Item Env:ICED_READER_OPEN -ErrorAction SilentlyContinue }

$proc = Start-Process -FilePath $Exe -PassThru
Write-Host "launch pid=$($proc.Id) exe=$Exe book=$Book"

$hwnd = [IntPtr]::Zero
$deadline = (Get-Date).AddSeconds(40)
while ((Get-Date) -lt $deadline) {
  Start-Sleep -Milliseconds 250
  $p = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
  if (-not $p) { throw "process exited during startup (exit=$($proc.ExitCode))" }
  $p.Refresh()
  if ($p.MainWindowHandle -ne 0) { $hwnd = $p.MainWindowHandle; break }
}
if ($hwnd -eq [IntPtr]::Zero) { throw "no main window after 40 s" }
$script:Hwnd = $hwnd
Write-Host "hwnd=$hwnd"

# Window position/size come from data/window.json on normal runs; force ours.
# The window may come up maximized (restored state), and MoveWindow is a no-op
# on a maximized window — restore first.
[void][W]::ShowWindow($hwnd, 9)
Start-Sleep -Milliseconds 400
[void][W]::MoveWindow($hwnd, 40, 40, $Width, $Height, $true)
Start-Sleep -Milliseconds 400
Focus-App

try {
  foreach ($step in ($Steps -split ';' | Where-Object { $_.Trim() } | ForEach-Object { $_.Trim() })) {
    $verb, $arg = $step.Split(':', 2)
    switch ($verb) {
      'sleep' { Start-Sleep -Milliseconds ([int]$arg) }
      'shot'  { Focus-App; Save-Shot $arg }
      'key'   { Focus-App; [System.Windows.Forms.SendKeys]::SendWait($arg); Start-Sleep -Milliseconds 150 }
      'type'  { Focus-App; [System.Windows.Forms.SendKeys]::SendWait($arg); Start-Sleep -Milliseconds 150 }
      'move'  {
        Focus-App
        $x, $y = $arg.Split(',') | ForEach-Object { [int]$_ }
        $org = Get-ClientOrigin $script:Hwnd
        [void][W]::SetCursorPos($org.X + $x, $org.Y + $y)
        Start-Sleep -Milliseconds 120
      }
      'click' {
        Focus-App
        $x, $y = $arg.Split(',') | ForEach-Object { [int]$_ }
        $org = Get-ClientOrigin $script:Hwnd
        [void][W]::SetCursorPos($org.X + $x, $org.Y + $y)
        Start-Sleep -Milliseconds 120
        [W]::mouse_event([W]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [W]::mouse_event([W]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 250
      }
      'dblclick' {
        Focus-App
        $x, $y = $arg.Split(',') | ForEach-Object { [int]$_ }
        $org = Get-ClientOrigin $script:Hwnd
        [void][W]::SetCursorPos($org.X + $x, $org.Y + $y)
        Start-Sleep -Milliseconds 120
        for ($i = 0; $i -lt 2; $i++) {
          [W]::mouse_event([W]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
          Start-Sleep -Milliseconds 40
          [W]::mouse_event([W]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
          Start-Sleep -Milliseconds 80
        }
        Start-Sleep -Milliseconds 250
      }
      'wheel' {
        Focus-App
        [W]::mouse_event([W]::MOUSEEVENTF_WHEEL, 0, 0, [int]$arg, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 350
      }
      'resize' {
        $w, $h = $arg.Split('x') | ForEach-Object { [int]$_ }
        [void][W]::MoveWindow($script:Hwnd, 40, 40, $w, $h, $true)
        Start-Sleep -Milliseconds 900
      }
      'maximize' { [void][W]::ShowWindow($script:Hwnd, 3); Start-Sleep -Milliseconds 900 }
      'restore'  { [void][W]::ShowWindow($script:Hwnd, 9); Start-Sleep -Milliseconds 900 }
      'dump' {
        $sz = Get-ClientSize $script:Hwnd
        $org = Get-ClientOrigin $script:Hwnd
        Write-Host "  dump $arg client=$($sz.W)x$($sz.H) origin=$($org.X),$($org.Y)"
      }
      'mem' {
        # App process + its WebView2 descendants: where the raster cache and the
        # decoded bitmaps actually live.
        $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, WorkingSetSize
        $ids = New-Object System.Collections.Generic.HashSet[int]
        [void]$ids.Add($proc.Id)
        $grew = $true
        while ($grew) {
          $grew = $false
          foreach ($p in $all) {
            if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) {
              [void]$ids.Add([int]$p.ProcessId); $grew = $true
            }
          }
        }
        $total = 0
        foreach ($p in $all) { if ($ids.Contains([int]$p.ProcessId)) { $total += [int64]$p.WorkingSetSize } }
        $app = ($all | Where-Object { $_.ProcessId -eq $proc.Id } | Select-Object -First 1).WorkingSetSize
        Write-Host ("  mem {0}: app={1:N1} MB, tree={2:N1} MB ({3} procs)" -f $arg, ($app / 1MB), ($total / 1MB), $ids.Count)
      }
      'close' {
        Focus-App
        [void]$proc.CloseMainWindow()
        if (-not $proc.WaitForExit(15000)) { Write-Host "  ! did not exit in 15 s" }
        else { Write-Host "  closed, exit=$($proc.ExitCode)" }
      }
      default { throw "unknown step '$step'" }
    }
  }
}
finally {
  if ($script:Hwnd -ne [IntPtr]::Zero) { [void][W]::DropTopmost($script:Hwnd) }
  if (-not $KeepOpen -and -not $proc.HasExited) {
    Write-Host "cleanup: killing pid=$($proc.Id)"
    $proc.Kill()
    $proc.WaitForExit(5000) | Out-Null
  }
  Remove-Item Env:ICED_READER_OPEN -ErrorAction SilentlyContinue
}
Write-Host "done"
