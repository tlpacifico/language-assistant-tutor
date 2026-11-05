<#
.SYNOPSIS
  Mede o tempo de transcrição e de voz em gravações indicadas pelo utilizador.

.DESCRIPTION
  Compara beam_size=5 (atual) com beam_size=1 (candidato) com o modelo small em
  CPU/int8. Por gravação e configuração faz uma passagem fria e cinco carregadas
  e guarda só durações (nunca o texto, nem caminhos) numa pasta local.

  Não varre as gravações pessoais da aplicação: indique explicitamente uma pasta
  com os WAV a medir. Os resultados não são enviados nem versionados.

.PARAMETER Recordings
  Pasta com os ficheiros .wav (só o nível superior).

.PARAMETER Out
  Pasta local de resultados. Por omissão, uma pasta temporária.

.PARAMETER VoiceText
  Texto opcional para medir o primeiro bloco de voz e a síntese completa.

.EXAMPLE
  .\scripts\benchmark-audio.ps1 -Recordings D:\Gravacoes\medicao
#>
param(
    [Parameter(Mandatory = $true)][string]$Recordings,
    [string]$Out = (Join-Path ([System.IO.Path]::GetTempPath()) 'article-benchmark'),
    [string]$VoiceText = 'Good point. Why does that matter for the way teams collaborate?'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$python = Join-Path $root '.venv\Scripts\python.exe'
if (-not (Test-Path $python)) {
    throw 'Falta o ambiente virtual (.venv). Siga docs/phase-1/setup.md.'
}
$recordingsPath = (Resolve-Path $Recordings).Path
$appData = Join-Path $env:APPDATA 'dev.thacio.languageassistanttutor'
if ($recordingsPath.StartsWith($appData, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Indique uma pasta de gravações de medição, não as sessões pessoais da aplicação.'
}
if (-not (Get-ChildItem -Path $recordingsPath -Filter *.wav -File)) {
    throw "Não há ficheiros .wav em $recordingsPath."
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$manifest = $env:LANGUAGE_TUTOR_MODELS_MANIFEST
$arguments = @('-m', 'audio_worker.benchmark', '--recordings', $recordingsPath, '--out', $Out)
if ($manifest) { $arguments += @('--manifest', $manifest) }
if ($VoiceText) { $arguments += @('--voice-text', $VoiceText) }

& $python @arguments
if ($LASTEXITCODE -ne 0) { throw "A medição terminou com o código $LASTEXITCODE." }
Write-Host "Resultados (só durações) em: $Out"
