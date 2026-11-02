# Verificação local da fase 1 (Windows + PowerShell 7).
#
# Executa, pela ordem do plano técnico, os checks automáticos do frontend, do
# núcleo Rust e do auxiliar Python. Interrompe no primeiro comando que termine
# com saída diferente de zero; só imprime sucesso quando todos terminarem com 0.
#
# Uso, a partir de qualquer diretório:
#   .\scripts\verify.ps1
#
# Pré-requisitos reais desta máquina:
#   - Node/npm no PATH (usar npm.cmd em PowerShell);
#   - Cargo em %USERPROFILE%\.cargo\bin (não está no PATH por omissão);
#   - .venv Python 3.11 na raiz, com o auxiliar instalado (ver docs/phase-1/setup.md).

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Mensagens com acentos legíveis quando a consola não está em UTF-8.
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

$repoRoot = Split-Path -Parent $PSScriptRoot
$python = Join-Path $repoRoot '.venv\Scripts\python.exe'
$manifest = Join-Path $repoRoot 'src-tauri\Cargo.toml'

if (-not (Test-Path $python)) {
    Write-Host "ERRO: falta $python. Criar a .venv com 'py -3.11 -m venv .venv' (ver docs/phase-1/setup.md)." -ForegroundColor Red
    exit 1
}

# O Cargo não está no PATH por omissão nesta máquina; documentado no README.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:Path = "$cargoBin;$env:Path"
    }
    else {
        Write-Host 'ERRO: cargo não encontrado no PATH nem em %USERPROFILE%\.cargo\bin.' -ForegroundColor Red
        exit 1
    }
}

$checks = @(
    @{ Name = 'TypeScript (tsc --noEmit)'; File = 'npm.cmd'; Arguments = @('run', 'typecheck') }
    @{ Name = 'Testes React (Vitest)'; File = 'npm.cmd'; Arguments = @('test') }
    @{ Name = 'Build do frontend (typecheck + Vite)'; File = 'npm.cmd'; Arguments = @('run', 'build') }
    @{ Name = 'Formatação Rust (cargo fmt --check)'; File = 'cargo'; Arguments = @('fmt', '--manifest-path', $manifest, '--check') }
    @{ Name = 'Lint Rust (cargo clippy -D warnings)'; File = 'cargo'; Arguments = @('clippy', '--manifest-path', $manifest, '--all-targets', '--', '-D', 'warnings') }
    @{ Name = 'Testes Rust (cargo test)'; File = 'cargo'; Arguments = @('test', '--manifest-path', $manifest) }
    @{ Name = 'Testes Python (pytest sem a marca models)'; File = $python; Arguments = @('-m', 'pytest', 'audio-worker/tests', '-q', '-m', 'not models') }
    @{ Name = 'Dependências Python (pip check)'; File = $python; Arguments = @('-m', 'pip', 'check') }
)

$started = Get-Date
$passed = 0

for ($index = 0; $index -lt $checks.Count; $index++) {
    $check = $checks[$index]
    $label = '[{0}/{1}] {2}' -f ($index + 1), $checks.Count, $check.Name
    Write-Host ''
    Write-Host $label -ForegroundColor Cyan
    Write-Host ('  > {0} {1}' -f $check.File, ($check.Arguments -join ' ')) -ForegroundColor DarkGray

    $checkStarted = Get-Date
    Push-Location $repoRoot
    try {
        & $check.File @($check.Arguments)
        $exitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }
    $elapsed = (Get-Date) - $checkStarted

    if ($exitCode -ne 0) {
        Write-Host ''
        Write-Host ("FALHOU: {0} terminou com saída {1} após {2:N1} s." -f $check.Name, $exitCode, $elapsed.TotalSeconds) -ForegroundColor Red
        Write-Host 'Verificação interrompida no primeiro erro; os checks seguintes não foram executados.' -ForegroundColor Red
        exit $exitCode
    }

    $passed++
    Write-Host ('  OK ({0:N1} s)' -f $elapsed.TotalSeconds) -ForegroundColor Green
}

$total = (Get-Date) - $started
Write-Host ''
Write-Host ('Verificação concluída: {0}/{1} checks com saída 0 em {2:N1} s.' -f $passed, $checks.Count, $total.TotalSeconds) -ForegroundColor Green
exit 0
