//! Fachada de consulta, apenas de leitura, sobre os treinos da fase 1.
//!
//! A árvore `phase1/` é conservada tal como está: a aplicação nova nunca lhe
//! escreve um byte (regra R8 da fase 2). Este módulo expõe só a listagem dos
//! treinos antigos, a leitura de um deles por identificador e a resolução do
//! áudio original para reprodução — a construção é a mesma de
//! [`crate::storage::AttemptStore`], que já sabe ler manifestos, transcrições e
//! revisões. Nenhum escritor é exposto no fluxo de execução.
//!
//! Regras aplicadas:
//!
//! - a listagem vem das pastas de `attempts/`; `last-attempt.json` não é um
//!   índice e pode estar apagado ou ilegível sem esconder os treinos;
//! - um treino inválido aparece como um item com avisos, sem interromper os
//!   restantes;
//! - identificadores e referências que tentem sair da pasta da resposta são
//!   recusados antes de qualquer acesso ao disco;
//! - nada é copiado para SQLite, movido ou apagado.

use std::path::{Path, PathBuf};

use crate::domain::{AppError, RestoreResult};
use crate::storage::{artifact_missing, resolve_reference, AttemptStore, ORIGINAL_FILE};

/// Consulta dos treinos guardados pela versão anterior da aplicação.
///
/// O tipo só guarda a raiz e não mantém descritores abertos; partilhá-lo entre
/// comandos é seguro e não existe nenhum método de escrita.
#[derive(Debug, Clone)]
pub struct LegacyStore {
    attempts: AttemptStore,
}

impl LegacyStore {
    /// Cria a fachada sobre `app_data_dir()/phase1`.
    ///
    /// Nada é criado nem escrito: a pasta é lida quando a listagem é pedida.
    pub fn new(root: PathBuf) -> Self {
        Self {
            attempts: AttemptStore::new(root),
        }
    }

    /// Lista os treinos anteriores, os válidos por data decrescente.
    ///
    /// Um treino corrompido produz `attempt: None` com os avisos do que falhou;
    /// os restantes continuam listados.
    pub fn list(&self) -> Result<Vec<RestoreResult>, AppError> {
        self.attempts.list_saved()
    }

    /// Lê um treino anterior pelo identificador, sem alterar o formato antigo.
    pub fn load(&self, attempt_id: &str) -> Result<RestoreResult, AppError> {
        self.attempts.load_saved(attempt_id)
    }

    /// Resolve o áudio original de um treino para reprodução.
    ///
    /// O caminho vem da referência relativa guardada no manifesto e nunca da
    /// interface; uma referência que saia da pasta da resposta é recusada. Um
    /// manifesto ilegível ou ausente não impede ouvir o original com o nome
    /// convencional da fase 1: o áudio e os textos são conservados de forma
    /// independente.
    pub fn resolve_original(&self, attempt_id: &str) -> Result<PathBuf, AppError> {
        // A pasta é validada antes de qualquer leitura do manifesto.
        let directory = self.attempts.attempt_directory(attempt_id)?;
        let reference = match self.load(attempt_id) {
            Ok(restored) => restored
                .attempt
                .map(|attempt| attempt.original_file)
                .unwrap_or_else(|| ORIGINAL_FILE.to_owned()),
            Err(_) => ORIGINAL_FILE.to_owned(),
        };
        self.resolve_in(&directory, &reference)
    }

    /// Resolve uma referência relativa dentro da pasta de um treino.
    ///
    /// Reutiliza a regra da persistência antiga: referências absolutas, `..` e
    /// prefixos são recusados, e o ficheiro tem de existir.
    pub(crate) fn resolve_reference(
        &self,
        attempt_id: &str,
        reference: &str,
    ) -> Result<PathBuf, AppError> {
        let directory = self.attempts.attempt_directory(attempt_id)?;
        self.resolve_in(&directory, reference)
    }

    fn resolve_in(&self, directory: &Path, reference: &str) -> Result<PathBuf, AppError> {
        if Path::new(reference).is_absolute() {
            return Err(invalid_reference());
        }
        let path = resolve_reference(directory, reference).ok_or_else(invalid_reference)?;
        if !path.is_file() {
            return Err(artifact_missing(
                "O ficheiro do treino não foi encontrado; os restantes dados foram conservados.",
            ));
        }
        Ok(path)
    }
}

/// Referência de ficheiro recusada por não ficar dentro da pasta do treino.
fn invalid_reference() -> AppError {
    artifact_missing("A referência do ficheiro do treino não é válida.")
}
