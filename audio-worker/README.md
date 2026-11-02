# audio-worker

Auxiliar local da fase 1: um processo Python 3.11 persistente que serve
transcrição (faster-whisper, modelo `small`, CPU/int8) e síntese (Kokoro-82M,
inglês americano, voz `af_heart`, WAV PCM16 mono a 24 kHz) por linhas JSON em
`stdin`/`stdout`.

## Instalação (Windows)

O PyTorch CPU tem de ser instalado primeiro, a partir do índice oficial, para
não trazer as rodas CUDA:

```powershell
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check
```

Versões fixadas: `faster-whisper==1.2.1`, `kokoro==0.9.4`, `misaki[en]==0.9.4`.
O conjunto resolvido (incluindo `torch==2.15.0+cpu`, `ctranslate2`, `spacy`,
`transformers`, `soundfile`, `espeakng-loader` e `pytest`) está registado em
`requirements.lock.txt`, gerado com `pip freeze --exclude-editable` depois de
`pip check` limpo. Para reconstruir o ambiente a partir do lock:

```powershell
.\.venv\Scripts\python.exe -m pip install torch==2.15.0 --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.lock.txt --extra-index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
```

### Recursos linguísticos

- **spaCy inglês (`en_core_web_sm`)**: exigido pelo `misaki.en.G2P`. Não é uma
  dependência pip do `requirements.in` porque é um recurso de dados;
  `misaki` tenta instalá-lo sozinho (`spacy.cli.download`) na primeira
  construção do `G2P`. Como isso é um download implícito durante a prática, o
  worker verifica o recurso antes de construir o pipeline e o
  `audio-worker-prepare` instala-o explicitamente
  (`.\.venv\Scripts\python.exe -m spacy download en_core_web_sm`).
- **eSpeak-NG**: o `misaki[en]` usa o fallback `misaki.espeak.EspeakFallback`
  para palavras fora do dicionário. No Windows **não** é preciso instalar o
  eSpeak-NG no sistema: o pacote `espeakng-loader` traz `espeak-ng.dll` e
  `espeak-ng-data` dentro do wheel. Validado nesta máquina com
  `EspeakFallback(british=False)("difficult")` → `dˈɪfɪkᵊlt`.

## Preparação offline dos modelos

O único comando que acede à rede. Descarrega `Systran/faster-whisper-small`, a
configuração/pesos `hexgrad/Kokoro-82M` e `voices/af_heart.pt`, instala o
recurso inglês do spaCy, valida o eSpeak-NG incluído, escreve o manifesto e faz
uma síntese fixa para revelar dependências tardias:

```powershell
.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "D:\Modelos de Voz\language-assistant-tutor"
```

O manifesto (`models.json`, por omissão em `<models-dir>/models.json`) usa
caminhos absolutos com espaços e contém `whisperDir`, `whisperModel`,
`kokoroConfig`, `kokoroWeights`, `kokoroVoice`, `revisions` e o inventário
`resources` (`spacyModel`, `spacyModelVersion`, `espeakLibrary`, `espeakData`,
`espeakFallback`). Os pesos ficam fora do repositório (`models/` é ignorado).

## Serviço

```powershell
.\.venv\Scripts\python.exe -u -m audio_worker --models-manifest "D:\Modelos de Voz\language-assistant-tutor\models.json"
```

- Uma mensagem JSON por linha, `protocolVersion: 1`, `id`, `op`; operações
  `health`, `transcribe`, `synthesize` e `shutdown`.
- `stdout` transporta apenas protocolo; o que as bibliotecas escrevem é enviado
  para `stderr`, que a app drena continuamente.
- `HF_HUB_OFFLINE=1` e `TRANSFORMERS_OFFLINE=1` são aplicados antes de qualquer
  import de motor, por isso a prática nunca descarrega nada.
- `health` verifica ficheiros, voz, dependências e recursos ingleses sem
  carregar modelos. `shutdown` responde `{"stopped": true}` e o processo sai.
- Os exemplos verificados das mensagens estão em
  [`contracts/worker-v1.examples.json`](../contracts/worker-v1.examples.json).

## Testes

```powershell
.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_protocol.py audio-worker/tests/test_engines.py -q
.\.venv\Scripts\python.exe -m pytest audio-worker/tests -q -m models
```

Os testes unitários não precisam de pesos. `test_offline_models.py` está marcado
com o marcador `models` e só corre depois de `audio-worker-prepare`; sem pesos
produz um *skip* explícito, nunca um sucesso falso.
