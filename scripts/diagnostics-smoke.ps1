[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$Executable)
# Executar a partir da raiz, apenas com o build isolado descrito em docs/diagnostics/validation.md.
$ErrorActionPreference = 'Stop'
$repoRoot = (Get-Location).Path
$smokeRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot 'tmp/diagnostics-smoke-data'))
$outputRoot = Join-Path $repoRoot 'tmp/diagnostics-work/webview'
New-Item -ItemType Directory -Force $outputRoot | Out-Null
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = [System.IO.Path]::GetFullPath($Executable)
$startInfo.UseShellExecute = $false
$startInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
$startInfo.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = "--remote-debugging-port=$port"
$startInfo.Environment['WEBVIEW2_USER_DATA_FOLDER'] = Join-Path $smokeRoot 'webview'
$startInfo.Environment['LANGUAGE_TUTOR_CONFIG'] = Join-Path $smokeRoot 'missing-audio.json'
$testProcess = [System.Diagnostics.Process]::Start($startInfo)
try {
    $ready = $false
    $until = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $until -and -not $testProcess.HasExited) {
        try {
            $null = Invoke-RestMethod -Uri "http://127.0.0.1:$port/json/version" -TimeoutSec 1
            $ready = $true
            break
        } catch { Start-Sleep -Milliseconds 200 }
    }
    if (-not $ready) { throw 'WebView2 não disponibilizou a depuração do ensaio.' }
    & python scripts/diagnostics-smoke.py --cdp "http://127.0.0.1:$port" --data-root $smokeRoot --output $outputRoot
    if ($LASTEXITCODE -ne 0) { throw 'O ensaio do painel falhou.' }
} finally {
    if (-not $testProcess.HasExited) {
        $closed = $testProcess.CloseMainWindow()
        if (-not $testProcess.WaitForExit(15000)) {
            $testProcess.Kill($true)
            throw 'O processo de ensaio não terminou após o pedido de fecho.'
        }
        Write-Output "CloseMainWindow=$closed ExitCode=$($testProcess.ExitCode)"
    }
    if ($testProcess.ExitCode -ne 0) { throw "Saída do ensaio: $($testProcess.ExitCode)" }
    $testProcess.Dispose()
}

