//! Artefactos de áudio das entrevistas guiadas: caminhos geridos, validação de
//! WAV, reserva de pastas e reconciliação no arranque.
//!
//! A raiz recebida por [`ArtifactStore::new`] é `app_data_dir()/sessions`; cada
//! gravação vive em `<raiz>/<sessão>/<pergunta-da-sessão>/<gravação>/` com
//! `original.partial.wav` (nunca promovido), `original.wav` (evidência durável)
//! e `normalized.wav` (derivado). Nenhum caminho é resolvido a partir de texto
//! do utilizador: só identificadores validados e a raiz configurada. Qualquer
//! componente `..`, caminho absoluto ou referência que saia da raiz é recusado.
//!
//! O `original.wav` finalizado é a evidência recuperável: a linha da gravação é
//! reservada antes da captura e, se a escrita seguinte em SQLite falhar, o
//! ficheiro fica e a publicação é repetida pela reconciliação — nunca é preciso
//! gravar outra vez. Não existe uma transação comum entre SQLite e ficheiros:
//! esta divisão está documentada no relatório da tarefa.
//!
//! A reconciliação inspeciona reservas pendentes, publica originais fechados
//! (marcando `incomplete` quando o fecho não teve metadados confirmados), marca
//! capturas sem original como interrompidas, encerra operações antigas em
//! `interrupted` e conserva órfãos desconhecidos com um aviso. Nunca elimina
//! ficheiros nem duplica linhas.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use hound::{SampleFormat, WavReader};

use crate::audio::{
    capture_interrupted, normalize_wav, CapturedAudio, RecordingTarget, ORIGINAL_FILE,
};
use crate::db::{Database, PendingRecording};
use crate::domain::{error_code, AppError, CaptureMeta};
use crate::interview::{RecordingState, RecordingView, RecoveryReport, Result, SessionDetail};

/// Nome do derivado normalizado dentro da pasta da gravação.
pub const NORMALIZED_FILE: &str = "normalized.wav";

/// Nível de pastas dentro da raiz gerida: sessão, pergunta da sessão, gravação.
const DEPTH: usize = 3;

/// Armazenamento dos ficheiros de áudio sobre a raiz `app_data_dir()/sessions`.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    /// Cria o armazenamento sobre uma raiz. Nada é escrito antes da primeira
    /// reserva.
    pub fn new(root: PathBuf) -> ArtifactStore {
        ArtifactStore { root }
    }

    /// Reserva a pasta de uma gravação já registada na base.
    ///
    /// Os três identificadores têm de ser nomes de pasta simples; um `..`, um
    /// caminho absoluto ou uma barra são recusados com `INVALID_INPUT`.
    pub fn reserve(
        &self,
        session_id: &str,
        session_question_id: &str,
        recording_id: &str,
    ) -> Result<RecordingTarget> {
        let directory = self.directory(session_id, session_question_id, recording_id)?;
        // Recusa uma ligação já existente no caminho antes de escrever através dela.
        self.ensure_no_links(&directory)?;
        fs::create_dir_all(&directory).map_err(|error| {
            AppError::new(
                error_code::STORAGE_WRITE_FAILED,
                format!("Não foi possível preparar a pasta da gravação: {error}."),
                true,
            )
        })?;
        Ok(RecordingTarget {
            id: recording_id.to_owned(),
            directory,
        })
    }

    /// Valida o `original.wav` finalizado de uma reserva.
    ///
    /// Um `original.partial.wav` nunca é lido nem promovido. Sem original, a
    /// captura foi interrompida e não existe duração nem sucesso a inventar.
    pub fn inspect(&self, target: &RecordingTarget) -> Result<CapturedAudio> {
        let directory = self.managed_directory(target)?;
        let original = directory.join(ORIGINAL_FILE);
        self.ensure_no_links(&original)?;
        let meta = inspect_file(&original)?;
        Ok(CapturedAudio {
            id: target.id.clone(),
            original,
            meta,
        })
    }

    /// Produz o derivado de transcrição a partir do original, sem o alterar.
    ///
    /// Reutiliza a normalização da fase 1 (`normalize_wav`); o derivado é
    /// escrito ao lado do original e a referência é registada pela base com
    /// [`Database::set_normalized`].
    pub fn normalize(&self, target: &RecordingTarget) -> Result<CaptureMeta> {
        let directory = self.managed_directory(target)?;
        let original = directory.join(ORIGINAL_FILE);
        let normalized = directory.join(NORMALIZED_FILE);
        self.ensure_no_links(&original)?;
        self.ensure_no_links(&normalized)?;
        if !original.is_file() {
            return Err(artifact_missing(format!(
                "O áudio original da gravação {} não foi encontrado para normalizar; \
                 a captura deve ser repetida.",
                target.id
            )));
        }
        normalize_wav(&original, &normalized)
    }

    /// Resolve uma referência relativa guardada na base, sem sair da raiz.
    pub fn resolve(&self, reference: &str) -> Result<PathBuf> {
        validate_reference(reference)?;
        let path = self.root.join(reference);
        self.ensure_no_links(&path)?;
        Ok(path)
    }

    /// Avisos por artefacto de uma gravação guardada.
    ///
    /// A base é a autoridade dos textos; esta verificação só acrescenta o que
    /// depende do disco. Um original em falta não esconde transcrições nem
    /// revisões.
    pub fn warnings(&self, recording: &RecordingView) -> Vec<AppError> {
        let mut warnings = Vec::new();
        if recording.state != RecordingState::Ready {
            return warnings;
        }
        match recording.original_file.as_deref() {
            Some(reference) => self.check(reference, "O áudio original", &mut warnings),
            None => warnings.push(artifact_missing(format!(
                "A gravação {} está marcada como pronta sem referência ao áudio original.",
                recording.id
            ))),
        }
        if let Some(reference) = recording.normalized_file.as_deref() {
            self.check(reference, "O áudio normalizado", &mut warnings);
        }
        warnings
    }

    /// Acrescenta aos detalhes da sessão os avisos de artefactos que dependem
    /// do disco, sem alterar os textos guardados.
    pub fn annotate(&self, detail: &mut SessionDetail) {
        for question in &mut detail.questions {
            if let Some(f) = &mut question.follow_up {
                for r in &mut f.recordings {
                    r.warnings = self.warnings(r);
                }
            }
            for recording in &mut question.recordings {
                let extra = self.warnings(recording);
                recording.warnings.extend(extra);
            }
        }
    }

    /// Pasta gerida de uma gravação, sem escrever nada.
    fn directory(
        &self,
        session_id: &str,
        session_question_id: &str,
        recording_id: &str,
    ) -> Result<PathBuf> {
        id_component(session_id, "da sessão")?;
        id_component(session_question_id, "da pergunta")?;
        id_component(recording_id, "da gravação")?;
        Ok(self
            .root
            .join(session_id)
            .join(session_question_id)
            .join(recording_id))
    }

    /// Confirma que o alvo recebido é exatamente uma pasta de gravação da raiz.
    fn managed_directory(&self, target: &RecordingTarget) -> Result<PathBuf> {
        id_component(&target.id, "da gravação")?;
        let relative = target
            .directory
            .strip_prefix(&self.root)
            .map_err(|_| outside_root(&target.directory))?;
        let parts: Vec<Component<'_>> = relative.components().collect();
        let managed = parts.len() == DEPTH
            && parts
                .iter()
                .all(|part| matches!(part, Component::Normal(_)));
        if !managed || parts[DEPTH - 1].as_os_str() != OsStr::new(&target.id) {
            return Err(outside_root(&target.directory));
        }
        self.ensure_no_links(&target.directory)?;
        Ok(target.directory.clone())
    }

    /// Recusa ligações (símbolos ou junções) abaixo da raiz gerida.
    ///
    /// Uma ligação criada em `<raiz>/<sessão>/<pergunta>/<gravação>` passaria a
    /// verificação lexical e faria ler ou escrever fora da pasta gerida. Cada
    /// componente existente é verificado; uma componente inexistente não pode
    /// redirecionar nada. Nunca segue a ligação para a inspecionar.
    fn ensure_no_links(&self, path: &Path) -> Result<()> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| outside_root(path))?;
        let mut current = self.root.clone();
        for component in relative.components() {
            let Component::Normal(_) = component else {
                return Err(outside_root(path));
            };
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if is_link(&metadata) => return Err(outside_root(path)),
                Ok(_) => {}
                // Nada existe neste nível (nem abaixo): não há ligação a seguir.
                Err(_) => return Ok(()),
            }
        }
        Ok(())
    }

    /// Verifica um ficheiro referenciado e acumula o aviso em vez de falhar.
    fn check(&self, reference: &str, label: &str, warnings: &mut Vec<AppError>) {
        match self.resolve(reference) {
            Ok(path) if path.is_file() => {}
            Ok(path) => warnings.push(artifact_missing(format!(
                "{label} não foi encontrado em {}; os textos e o histórico desta \
                 gravação continuam legíveis.",
                path.display()
            ))),
            Err(_) => warnings.push(artifact_missing(format!(
                "A referência guardada para {label} não é válida e o ficheiro não é lido."
            ))),
        }
    }

    /// Pastas de gravação que existem no disco sem linha correspondente na base.
    fn orphan_warnings(&self, known: &HashSet<(String, String, String)>) -> Vec<AppError> {
        let mut warnings = Vec::new();
        for session in subdirectories(&self.root) {
            for question in subdirectories(&session) {
                for recording in subdirectories(&question) {
                    let key = (
                        folder_name(&session),
                        folder_name(&question),
                        folder_name(&recording),
                    );
                    if !known.contains(&key) {
                        warnings.push(capture_interrupted(format!(
                            "Existe áudio guardado em {} sem gravação associada na base; \
                             foi conservado sem alterações e não foi publicado.",
                            recording.display()
                        )));
                    }
                }
            }
        }
        warnings
    }
}

/// Referência relativa do original de uma gravação.
pub fn original_reference(
    session_id: &str,
    session_question_id: &str,
    recording_id: &str,
) -> Result<String> {
    reference(session_id, session_question_id, recording_id, ORIGINAL_FILE)
}

/// Referência relativa do derivado normalizado de uma gravação.
pub fn normalized_reference(
    session_id: &str,
    session_question_id: &str,
    recording_id: &str,
) -> Result<String> {
    reference(
        session_id,
        session_question_id,
        recording_id,
        NORMALIZED_FILE,
    )
}

/// Valida uma referência relativa guardada na base.
///
/// Só aceita componentes normais: recusa caminhos absolutos, prefixos de
/// unidade, `..`, `.` e referências vazias.
pub fn validate_reference(reference: &str) -> Result<()> {
    let path = Path::new(reference);
    if reference.is_empty() || path.is_absolute() {
        return Err(invalid_reference(reference));
    }
    let mut components = 0;
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(invalid_reference(reference));
        }
        components += 1;
    }
    if components == 0 {
        return Err(invalid_reference(reference));
    }
    Ok(())
}

/// Lê e valida um WAV finalizado, devolvendo o que está efetivamente escrito.
///
/// A leitura completa deteta um cabeçalho truncado ou ilegível; a duração vem
/// das amostras reais, nunca de um valor assumido.
pub fn inspect_file(path: &Path) -> Result<CaptureMeta> {
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            artifact_missing(format!(
                "O áudio original não foi encontrado em {}; a captura foi \
                 interrompida e não existe uma resposta finalizada.",
                path.display()
            ))
        } else {
            io_failure(format!("Não foi possível ler o áudio original: {error}."))
        }
    })?;
    if !metadata.is_file() {
        return Err(artifact_missing(format!(
            "O áudio original em {} não é um ficheiro.",
            path.display()
        )));
    }

    let mut reader = WavReader::open(path).map_err(|error| match error {
        // Só os bloqueios de acesso transitórios do Windows são repetíveis:
        // partilha (32), acesso negado (5) e bloqueio (33). O Hound também
        // devolve `IoError` sintético no fim de ficheiro («Failed to read enough
        // bytes»), sem código do sistema: um cabeçalho truncado é permanente.
        hound::Error::IoError(error) if is_transient_io(&error) => io_failure(format!(
            "Não foi possível abrir o áudio original neste momento: {error}."
        )),
        error => invalid_audio(format!("O áudio original não é um WAV legível: {error}.")),
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(invalid_audio(
            "O áudio original tem um formato inválido (canais ou taxa a zero).",
        ));
    }
    if spec.sample_format != SampleFormat::Int {
        return Err(invalid_audio(
            "O áudio original usa um formato de amostras não suportado.",
        ));
    }

    let mut samples = 0u64;
    for sample in reader.samples::<i32>() {
        sample.map_err(|error| {
            invalid_audio(format!(
                "O áudio original está truncado ou ilegível: {error}."
            ))
        })?;
        samples += 1;
    }
    if samples == 0 {
        return Err(invalid_audio(
            "A gravação não contém amostras de áudio e não pode ser guardada como resposta.",
        ));
    }

    let frames = samples / u64::from(spec.channels);
    Ok(CaptureMeta {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        duration_ms: duration_ms(frames, spec.sample_rate),
        bytes: metadata.len(),
        incomplete: false,
    })
}

/// Recupera o estado deixado por um encerramento abrupto.
///
/// Publica os originais fechados que ficaram pendentes (sem nova captura),
/// marca como interrompidas as reservas sem original, encerra operações antigas
/// em `interrupted` e conserva órfãos com aviso. Correr duas vezes seguidas não
/// produz linhas novas nem elimina ficheiros.
pub fn reconcile(database: &Database, artifacts: &ArtifactStore) -> Result<RecoveryReport> {
    let mut report = RecoveryReport {
        published: Vec::new(),
        warnings: Vec::new(),
        interrupted_operations: Vec::new(),
    };

    let pending: Vec<PendingRecording> = database.pending_recordings()?;
    for reservation in pending {
        let target = match artifacts.directory(
            &reservation.session_id,
            &reservation.session_question_id,
            &reservation.id,
        ) {
            Ok(directory) => RecordingTarget {
                id: reservation.id.clone(),
                directory,
            },
            Err(error) => {
                report.warnings.push(error);
                continue;
            }
        };

        let capture = match artifacts.inspect(&target) {
            Ok(capture) => capture,
            // Um bloqueio ou uma falha de acesso é transitório: a reserva fica
            // pendente e o próximo arranque (ou uma repetição do serviço) volta
            // a publicar o mesmo ficheiro, sem nova captura.
            Err(error) if error.retryable => {
                report.warnings.push(error);
                continue;
            }
            // Só uma condição permanente (original em falta, WAV inválido ou sem
            // amostras) torna a captura irrecuperável. O ficheiro nunca é
            // eliminado, mesmo neste caso.
            Err(error) => {
                database.mark_interrupted(&reservation.id)?;
                report
                    .warnings
                    .push(if error.code == error_code::ARTIFACT_MISSING {
                        interrupted_capture(&reservation.id)
                    } else {
                        error
                    });
                continue;
            }
        };

        // O WAV está fechado, mas o fecho que o serviço confirmou perdeu-se com
        // a falha: a publicação recuperada é marcada incompleta.
        let capture = CapturedAudio {
            meta: CaptureMeta {
                incomplete: true,
                ..capture.meta
            },
            ..capture
        };
        match database.publish_recording(capture) {
            Ok(_) => report.published.push(reservation.id),
            Err(error) => report.warnings.push(error),
        }
    }

    let known: HashSet<(String, String, String)> = database
        .recording_locations()?
        .into_iter()
        .map(|location| {
            (
                location.session_id,
                location.session_question_id,
                location.id,
            )
        })
        .collect();
    report.warnings.extend(artifacts.orphan_warnings(&known));

    report.interrupted_operations = database.interrupt_running_operations()?;
    Ok(report)
}

/// Referência relativa de um ficheiro dentro da pasta de uma gravação.
fn reference(
    session_id: &str,
    session_question_id: &str,
    recording_id: &str,
    file: &str,
) -> Result<String> {
    id_component(session_id, "da sessão")?;
    id_component(session_question_id, "da pergunta")?;
    id_component(recording_id, "da gravação")?;
    Ok(format!(
        "{session_id}/{session_question_id}/{recording_id}/{file}"
    ))
}

/// Um identificador só pode ser um nome de pasta simples.
fn id_component<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    let mut components = Path::new(value).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(value),
        _ => Err(AppError::new(
            error_code::INVALID_INPUT,
            format!("O identificador {label} não é um nome de pasta válido."),
            false,
        )),
    }
}

fn duration_ms(frames: u64, sample_rate: u32) -> u64 {
    let rate = u64::from(sample_rate);
    (frames * 1_000 + rate / 2) / rate
}

fn subdirectories(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect()
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn invalid_reference(reference: &str) -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        format!("A referência de áudio «{reference}» não é relativa à pasta gerida."),
        false,
    )
}

fn outside_root(directory: &Path) -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        format!(
            "A pasta {} não pertence à pasta gerida dos artefactos de áudio.",
            directory.display()
        ),
        false,
    )
}

fn invalid_audio(message: impl Into<String>) -> AppError {
    AppError::new(error_code::INVALID_AUDIO, message, false)
}

/// Falha de acesso ao ficheiro que pode desaparecer numa nova tentativa.
fn io_failure(message: impl Into<String>) -> AppError {
    AppError::new(error_code::STORAGE_WRITE_FAILED, message, true)
}

/// Apenas os bloqueios de acesso transitórios do Windows são repetíveis.
///
/// Códigos do sistema: 32 (partilha), 5 (acesso negado) e 33 (bloqueio). Todos
/// os outros erros de I/O — incluindo o erro sintético do Hound para o fim de
/// ficheiro, que não tem código do sistema — são permanentes.
fn is_transient_io(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32 | 5 | 33))
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        false
    }
}

/// Ligações simbólicas e junções do Windows (reparse points).
fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // `FILE_ATTRIBUTE_REPARSE_POINT`: cobre junções, além dos símbolos.
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn artifact_missing(message: impl Into<String>) -> AppError {
    AppError::new(error_code::ARTIFACT_MISSING, message, false)
}

/// Captura sem original finalizado: terminal, não repetível pelo mesmo pedido.
///
/// A recuperação é gravar de novo; distinguir isto de um bloqueio transitório
/// permite ao serviço decidir entre «tentar mais tarde» e «captura perdida».
fn interrupted_capture(recording_id: &str) -> AppError {
    AppError::new(
        error_code::CAPTURE_INTERRUPTED,
        format!(
            "A captura da gravação {recording_id} foi interrompida e não deixou áudio \
             original finalizado; a resposta não foi publicada e pode ser gravada de novo."
        ),
        false,
    )
}
