# Smoke test for lv on Windows.
# Installs via NSIS installer, launches the app with test fixtures,
# takes screenshots, quits, then uninstalls.
#
# Usage: pwsh scripts/smoke-test-windows.ps1 [-UpdateReference] [-Installer PATH]
#
# Screenshots saved to test/screenshots/actual/

param(
    [switch]$UpdateReference,
    [string]$Installer = ""
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

$FixturesDir = Join-Path $PWD "test/fixtures"
$ActualDir   = Join-Path $PWD "test/screenshots/actual"
$RefDir      = Join-Path $PWD "test/screenshots/reference"

# ── Find installer ───────────────────────────────────────────────────────
if (-not $Installer) {
    $Installer = (Get-ChildItem "build-installer/lv-setup-*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1).FullName
}
if (-not $Installer -or -not (Test-Path $Installer)) {
    Write-Error "Installer not found. Pass -Installer PATH or place in build-installer/"
    exit 1
}

# ── Install silently ─────────────────────────────────────────────────────
$InstallDir = Join-Path $env:LOCALAPPDATA "lv"
Write-Host "=== lv smoke test (Windows) ==="
Write-Host "Installer: $Installer"
Write-Host "InstallDir: $InstallDir"

$installProc = Start-Process -FilePath $Installer -ArgumentList "/S" -PassThru -Wait
if ($installProc.ExitCode -ne 0) {
    Write-Error "Installer failed with exit code $($installProc.ExitCode)"
    exit 1
}

$Binary = Join-Path $InstallDir "lv-imgui.exe"
if (-not (Test-Path $Binary)) {
    Write-Error "Binary not found after install: $Binary"
    Get-ChildItem $InstallDir -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "  $($_.Name)" }
    exit 1
}
Write-Host "Installed OK: $Binary"

# ── Mesa software OpenGL (for headless CI) ─────────────────────────────
if ($env:MESA_DLLS -and (Test-Path $env:MESA_DLLS)) {
    Write-Host "Copying Mesa software OpenGL DLLs..."
    Copy-Item (Join-Path $env:MESA_DLLS "*.dll") $InstallDir -Force
    Get-ChildItem $InstallDir -Filter "*.dll" | ForEach-Object { Write-Host "  $($_.Name)" }
}

# ── Generate test fixtures ───────────────────────────────────────────────
if (-not (Test-Path (Join-Path $FixturesDir "red_800x600.png"))) {
    Write-Host "Generating test fixtures..."
    python3 (Join-Path $FixturesDir "generate.py")
}

New-Item -ItemType Directory -Force -Path $ActualDir | Out-Null

# ── Isolated DB ──────────────────────────────────────────────────────────
$TmpDir = Join-Path $env:TEMP "lv-smoke-$(Get-Random)"
New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
$DbPath = Join-Path $TmpDir "lv-smoke.db"
$env:LV_DB_PATH = $DbPath

Write-Host "DB:       $DbPath"
Write-Host "Fixtures: $FixturesDir"
Write-Host ""

$trackStderr = Join-Path $TmpDir "track-stderr.log"
$trackProc = Start-Process -FilePath $Binary -ArgumentList "track", $FixturesDir `
    -RedirectStandardError $trackStderr -PassThru -Wait -WindowStyle Hidden
if (Test-Path $trackStderr) {
    Get-Content $trackStderr | ForEach-Object { Write-Host "  [track] $_" }
}
$imgCount = (Get-ChildItem $FixturesDir -Filter "*.png").Count
Write-Host "Tracked $imgCount test images"

# ── .NET helpers for screenshots and window management ───────────────────
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public class Win32 {
    [DllImport("user32.dll")]
    public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);

    [DllImport("user32.dll")]
    public static extern int GetWindowTextLength(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    public const uint WM_KEYDOWN = 0x0100;
    public const uint WM_KEYUP   = 0x0101;
    public const uint WM_CLOSE   = 0x0010;

    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left, Top, Right, Bottom;
    }
}
"@

function Find-LvWindow {
    param([int]$ProcessId)
    try {
        $p = Get-Process -Id $ProcessId -ErrorAction Stop
        if ($p.MainWindowHandle -ne [IntPtr]::Zero) {
            return $p.MainWindowHandle
        }
    } catch {}
    return $null
}

function Take-Screenshot {
    param([IntPtr]$hWnd, [string]$OutPath)
    $rect = New-Object Win32+RECT
    [Win32]::GetWindowRect($hWnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left
    $h = $rect.Bottom - $rect.Top
    if ($w -le 0 -or $h -le 0) {
        Write-Warning "Window has zero size, skipping screenshot"
        return
    }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $gfx.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $gfx.Dispose()
    $bmp.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "  screenshot: $([System.IO.Path]::GetFileName($OutPath))"
}

function Send-Key {
    param([IntPtr]$hWnd, [int]$VK)
    [Win32]::SetForegroundWindow($hWnd) | Out-Null
    Start-Sleep -Milliseconds 50
    [Win32]::PostMessage($hWnd, [Win32]::WM_KEYDOWN, [IntPtr]$VK, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 30
    [Win32]::PostMessage($hWnd, [Win32]::WM_KEYUP, [IntPtr]$VK, [IntPtr]::Zero) | Out-Null
}

# Virtual key codes
$VK_J = 0x4A
$VK_Q = 0x51

# ── Launch app ───────────────────────────────────────────────────────────
$stderrLog = Join-Path $TmpDir "stderr.log"
$proc = Start-Process -FilePath $Binary -ArgumentList $FixturesDir `
    -RedirectStandardError $stderrLog -PassThru -WindowStyle Normal

Write-Host "Launched lv (PID $($proc.Id))"

# ── Wait for window ─────────────────────────────────────────────────────
$hWnd = $null
for ($i = 0; $i -lt 100; $i++) {
    if ($proc.HasExited) {
        Write-Host "App exited early (code $($proc.ExitCode)) after $([math]::Round(($proc.ExitTime - $proc.StartTime).TotalMilliseconds))ms"
        Write-Host "--- stderr ---"
        if (Test-Path $stderrLog) { Get-Content $stderrLog | ForEach-Object { Write-Host "  $_" } }
        Write-Error "FATAL: app exited before creating a window"
        exit 1
    }
    $hWnd = Find-LvWindow -ProcessId $proc.Id
    if ($hWnd -ne $null) { break }
    Start-Sleep -Milliseconds 100
}

if ($hWnd -eq $null) {
    Write-Host "--- stderr ---"
    if (Test-Path $stderrLog) { Get-Content $stderrLog | ForEach-Object { Write-Host "  $_" } }
    Write-Error "FATAL: lv window did not appear within 10s"
    $proc.Kill()
    exit 1
}
Write-Host "  Window found: $hWnd"
Start-Sleep -Milliseconds 500  # let first frame render

# ── Screenshot 1: initial state ──────────────────────────────────────────
Write-Host ""
Write-Host "--- Screenshot 1: initial state ---"
$t0 = [System.Diagnostics.Stopwatch]::StartNew()
Take-Screenshot -hWnd $hWnd -OutPath (Join-Path $ActualDir "01_initial.png")
Write-Host "  Capture time: $($t0.ElapsedMilliseconds)ms"

# ── Navigate: j (next) ──────────────────────────────────────────────────
Write-Host ""
Write-Host "--- Navigate: j (next) ---"
Send-Key -hWnd $hWnd -VK $VK_J
Start-Sleep -Milliseconds 500

# ── Screenshot 2: after navigation ──────────────────────────────────────
Write-Host "--- Screenshot 2: after j ---"
Take-Screenshot -hWnd $hWnd -OutPath (Join-Path $ActualDir "02_after_nav.png")

# ── Wait 5 seconds ──────────────────────────────────────────────────────
Write-Host ""
Write-Host "--- Waiting 5s... ---"
Start-Sleep -Seconds 5

# ── Screenshot 3: after 5s idle ─────────────────────────────────────────
Write-Host "--- Screenshot 3: after 5s idle ---"
Take-Screenshot -hWnd $hWnd -OutPath (Join-Path $ActualDir "03_after_5s.png")

# ── Navigate: j twice ───────────────────────────────────────────────────
Write-Host ""
Write-Host "--- Navigate: j j (skip 2) ---"
Send-Key -hWnd $hWnd -VK $VK_J
Start-Sleep -Milliseconds 300
Send-Key -hWnd $hWnd -VK $VK_J
Start-Sleep -Milliseconds 500

# ── Screenshot 4: different image ───────────────────────────────────────
Write-Host "--- Screenshot 4: after 2x j ---"
Take-Screenshot -hWnd $hWnd -OutPath (Join-Path $ActualDir "04_navigated.png")

# ── Quit ─────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "--- Sending q to quit ---"
$tQuit = [System.Diagnostics.Stopwatch]::StartNew()
Send-Key -hWnd $hWnd -VK $VK_Q

# Wait for process to exit
$exited = $proc.WaitForExit(10000)
$quitMs = $tQuit.ElapsedMilliseconds

if (-not $exited) {
    Write-Warning "App did not exit within 10s, killing"
    $proc.Kill()
}
Write-Host "  Quit time: ${quitMs}ms"

if ($quitMs -gt 2000) {
    Write-Warning "SLOW QUIT: ${quitMs}ms (>2s)"
}

# ── Stderr log ───────────────────────────────────────────────────────────
Write-Host ""
Write-Host "--- App stderr (last 20 lines) ---"
if (Test-Path $stderrLog) {
    Get-Content $stderrLog -Tail 20 | ForEach-Object { Write-Host "  $_" }
}

# ── Update reference ────────────────────────────────────────────────────
if ($UpdateReference) {
    Write-Host ""
    Write-Host "--- Updating reference screenshots ---"
    New-Item -ItemType Directory -Force -Path $RefDir | Out-Null
    Copy-Item (Join-Path $ActualDir "*.png") $RefDir -Force
    $count = (Get-ChildItem $RefDir -Filter "*.png").Count
    Write-Host "  Copied $count screenshots to reference/"
}

# ── Visual diff ──────────────────────────────────────────────────────────
Write-Host ""
Write-Host "--- Visual diff (actual vs reference) ---"
$hasDiff = $false
$refImages = Get-ChildItem $RefDir -Filter "*.png" -ErrorAction SilentlyContinue

if ($refImages -and $refImages.Count -gt 0) {
    foreach ($ref in $refImages) {
        $actual = Join-Path $ActualDir $ref.Name
        if (-not (Test-Path $actual)) {
            Write-Host "  MISSING: $($ref.Name) (no actual screenshot)"
            $hasDiff = $true
            continue
        }
        # Byte-level comparison (fast, deterministic for identical renders)
        $refBytes = [System.IO.File]::ReadAllBytes($ref.FullName)
        $actBytes = [System.IO.File]::ReadAllBytes($actual)
        if ([System.Linq.Enumerable]::SequenceEqual($refBytes, $actBytes)) {
            Write-Host "  OK: $($ref.Name) - identical"
        } else {
            Write-Host "  DIFF: $($ref.Name) - differs"
            $hasDiff = $true
        }
    }
} else {
    Write-Host "  No reference screenshots yet. Run with -UpdateReference first."
}

# ── Uninstall ────────────────────────────────────────────────────────────
$Uninstaller = Join-Path $InstallDir "uninstall.exe"
if (Test-Path $Uninstaller) {
    Write-Host "--- Uninstalling ---"
    Start-Process -FilePath $Uninstaller -ArgumentList "/S" -Wait
    Write-Host "  Uninstalled"
}

# ── Cleanup ──────────────────────────────────────────────────────────────
Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue

Write-Host ""
if ($hasDiff) {
    Write-Host "RESULT: Visual differences detected"
    Write-Host "  Check test/screenshots/actual/ for current screenshots"
    exit 1
} else {
    Write-Host "RESULT: All screenshots match (or no reference yet)"
}
