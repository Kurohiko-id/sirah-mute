# Acceptance measurement for kuromute v2 (PowerShell 5.1, no admin, no modules).
# -LatencyOnly skips the process check and only reports latency from the log.
param(
    [string]$Exe = 'D:\dev\cargo-target\release\kuromute.exe',
    [int]$Toggle = 0,
    [string]$Log = (Join-Path $env:APPDATA 'MuteMic\logs\kuromute.log'),
    [switch]$LatencyOnly
)

function Get-Pct($sorted, $p) {
    # nearest-rank percentile on an ascending array
    $i = [math]::Ceiling($p / 100.0 * $sorted.Count) - 1
    $sorted[[math]::Max(0, [int]$i)]
}

function Show-Latency {
    $files = @(Get-ChildItem -Path "$Log*" -ErrorAction SilentlyContinue)
    $vals = @()
    foreach ($f in $files) {
        foreach ($m in (Select-String -Path $f.FullName -Pattern 'latency_ms=(\d+(?:\.\d+)?)' -AllMatches)) {
            foreach ($x in $m.Matches) {
                $vals += [double]::Parse($x.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
            }
        }
    }
    if ($vals.Count -eq 0) { Write-Host 'latency: no samples (no latency_ms= lines in log)'; return }
    $s = $vals | Sort-Object
    $p50 = Get-Pct $s 50
    $p95 = Get-Pct $s 95
    $v = if ($p95 -lt 16) { 'PASS' } else { 'FAIL' }
    Write-Host ('latency p50={0} ms  p95={1} ms  [{2} vs <16 ms]  samples={3}' -f $p50, $p95, $v, $s.Count)
}

if ($LatencyOnly) { Show-Latency; exit 0 }

if ($Toggle -gt 0) {
    if ($Toggle % 2 -ne 0) { Write-Host '-Toggle must be even so the mic ends in its starting state'; exit 1 }
    for ($i = 0; $i -lt $Toggle; $i++) {
        & $Exe --toggle | Out-Null  # GUI-subsystem exe: piping makes PowerShell wait
        if ($LASTEXITCODE -ne 0) {
            Write-Host "toggle $($i + 1)/$Toggle failed (exit $LASTEXITCODE): core not running?"
            exit 1
        }
        Start-Sleep -Milliseconds 50
    }
    Write-Host "toggled $Toggle times"
}

$p = Get-Process -Name kuromute -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Host 'kuromute is not running'; exit 1 }

Add-Type -Namespace Win -Name Gui -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetGuiResources(System.IntPtr h, uint flags);'

$mb = $p.PrivateMemorySize64 / 1MB
$v = if ($mb -lt 10) { 'PASS' } else { 'FAIL' }
Write-Host ('pid={0}' -f $p.Id)
Write-Host ('private memory: {0:N2} MB  [{1} vs <10 MB]' -f $mb, $v)
Write-Host ('handles: {0}' -f $p.HandleCount)
Write-Host ('GDI objects: {0}' -f [Win.Gui]::GetGuiResources($p.Handle, 0))
Write-Host ('USER objects: {0}' -f [Win.Gui]::GetGuiResources($p.Handle, 1))
Show-Latency
