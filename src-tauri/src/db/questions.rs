//! Banco de perguntas editável: banco inicial versionado e operações síncronas.
//!
//! O conteúdo inicial vem de `seed-v1.json`, incluído no binário e validado ao
//! ser carregado: 20 perguntas com identificadores fixos, distribuídas 4/3/3/4/3/3
//! pelas seis categorias. O seed só é inserido quando `content_seeds` ainda não
//! tem a versão `questions-v1`, dentro de uma transação que também escreve essa
//! marca no fim. Reabrir a aplicação não repõe perguntas editadas nem duplica
//! linhas; não existe caminho de eliminação nem `INSERT OR REPLACE`.
//!
//! As edições são otimistas: a versão esperada faz parte da condição do
//! `UPDATE` e uma versão antiga é recusada sem escrever nada.

use std::collections::HashSet;

use rusqlite::{params, Connection, Row};
use serde::Deserialize;
use uuid::Uuid;

use super::Database;
use crate::domain::{error_code, AppError};
use crate::interview::{validate_question, Category, Id, Question, QuestionInput, Result, Version};

/// Versão de conteúdo do banco inicial, gravada em `content_seeds`.
pub const QUESTIONS_SEED_VERSION: &str = "questions-v1";

/// Versão dos critérios STAR comuns abaixo. A cópia de cada sessão guarda os
/// critérios efetivos; esta versão documenta quando a redação comum mudou.
pub const STAR_CRITERIA_VERSION: u32 = 1;

/// Critérios STAR comuns, copiados para cada pergunta de uma sessão pela
/// criação da sessão (tarefa 3), junto com os critérios específicos editáveis.
pub const STAR_CRITERIA: [&str; 4] = [
    "Contexto: explicar a situação e o contexto da história.",
    "Responsabilidade individual: distinguir o papel próprio do trabalho da equipa.",
    "Ações: descrever as ações concretas tomadas.",
    "Resultado ou aprendizagem: explicar o desfecho ou a aprendizagem.",
];

/// Quantidade de perguntas do banco inicial.
const SEED_QUESTION_COUNT: usize = 20;

/// Distribuição da spec pelas seis categorias, por ordem de apresentação.
const SEED_DISTRIBUTION: [(Category, usize); 6] = [
    (Category::ConflictCollaboration, 4),
    (Category::LeadershipInitiative, 3),
    (Category::FailureLearning, 3),
    (Category::ProblemSolving, 4),
    (Category::PrioritiesDeadlines, 3),
    (Category::ChangeAdaptation, 3),
];

/// Banco inicial, lido do ficheiro no momento da compilação.
const SEED_V1_JSON: &str = include_str!("seed-v1.json");

/// Forma do ficheiro `seed-v1.json`.
#[derive(Debug, Deserialize)]
struct SeedFile {
    version: String,
    questions: Vec<SeedQuestion>,
}

/// Pergunta do banco inicial, sem versão nem datas: são atribuídas na inserção.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedQuestion {
    id: Id,
    text: String,
    category: Category,
    competencies: Vec<String>,
    criteria: Vec<String>,
}

impl Database {
    /// Lista o banco de perguntas, opcionalmente filtrado por categoria.
    ///
    /// A ordem é a de inserção, para a interface manter a ordem do banco
    /// inicial e das perguntas criadas pelo utilizador.
    pub fn list_questions(&self, category: Option<Category>) -> Result<Vec<Question>> {
        self.with_connection(|connection| match category {
            Some(category) => {
                let filter: [&dyn rusqlite::ToSql; 1] = [&category.as_str()];
                query_questions(
                    connection,
                    "SELECT id, text, category, competencies_json, criteria_json,
                            version, created_at, updated_at
                     FROM questions
                     WHERE category = ?1
                     ORDER BY rowid",
                    &filter,
                )
            }
            None => query_questions(
                connection,
                "SELECT id, text, category, competencies_json, criteria_json,
                        version, created_at, updated_at
                 FROM questions
                 ORDER BY rowid",
                &[],
            ),
        })
    }

    /// Cria uma pergunta editável, já validada e aparada nas pontas.
    pub fn create_question(&self, input: QuestionInput) -> Result<Question> {
        let input = validate_question(input)?;
        let competencies = json_array(&input.competencies)?;
        let criteria = json_array(&input.criteria)?;
        let id = Uuid::new_v4().to_string();
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection.transaction().map_err(write_failed)?;
            transaction
                .execute(
                    "INSERT INTO questions
                       (id, text, category, competencies_json, criteria_json,
                        version, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
                    params![
                        id,
                        input.text,
                        input.category.as_str(),
                        competencies,
                        criteria,
                        now
                    ],
                )
                .map_err(write_failed)?;
            // A leitura pertence à mesma transação: se devolver erro, a
            // pergunta não fica criada e uma repetição do pedido não duplica.
            let question = find_question(&transaction, &id)?
                .ok_or_else(|| read_corrupt("pergunta recém-criada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(question)
        })
    }

    /// Edita uma pergunta existente com controlo otimista de versão.
    ///
    /// Uma versão antiga (ou um identificador desconhecido) devolve
    /// `STALE_SOURCE` sem escrever nada. Uma edição bem-sucedida acrescenta uma
    /// versão e atualiza `updatedAt`.
    pub fn update_question(
        &self,
        id: &str,
        expected_version: Version,
        input: QuestionInput,
    ) -> Result<Question> {
        let input = validate_question(input)?;
        let competencies = json_array(&input.competencies)?;
        let criteria = json_array(&input.criteria)?;
        let expected_version = i64::try_from(expected_version).map_err(|_| stale_edit())?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection.transaction().map_err(write_failed)?;
            let affected = transaction
                .execute(
                    "UPDATE questions
                     SET text = ?1,
                         category = ?2,
                         competencies_json = ?3,
                         criteria_json = ?4,
                         version = version + 1,
                         updated_at = ?5
                     WHERE id = ?6 AND version = ?7",
                    params![
                        input.text,
                        input.category.as_str(),
                        competencies,
                        criteria,
                        now,
                        id,
                        expected_version
                    ],
                )
                .map_err(write_failed)?;
            if affected == 0 {
                // A transação termina sem commit; nada é escrito.
                return Err(stale_edit());
            }
            let question =
                find_question(&transaction, id)?.ok_or_else(|| read_corrupt("pergunta editada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(question)
        })
    }

    /// Aplica o banco inicial uma única vez, dentro de uma transação.
    ///
    /// É idempotente: quando `questions-v1` já consta em `content_seeds` nada é
    /// escrito, pelo que reabrir a aplicação não repõe perguntas editadas. A
    /// marca só é inserida depois de todas as perguntas; uma falha intermédia
    /// reverte as inserções e a marca.
    pub fn ensure_question_seed(&self) -> Result<()> {
        let questions = load_seed()?;

        self.with_connection(|connection| {
            let applied: bool = connection
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM content_seeds WHERE version = ?1)",
                    [QUESTIONS_SEED_VERSION],
                    |row| row.get(0),
                )
                .map_err(seed_failed)?;
            if applied {
                return Ok(());
            }

            let transaction = connection.transaction().map_err(seed_failed)?;
            let now = utc_now();
            for question in &questions {
                let competencies = json_array(&question.competencies)?;
                let criteria = json_array(&question.criteria)?;
                transaction
                    .execute(
                        "INSERT INTO questions
                           (id, text, category, competencies_json, criteria_json,
                            version, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
                        params![
                            question.id,
                            question.text,
                            question.category.as_str(),
                            competencies,
                            criteria,
                            now
                        ],
                    )
                    .map_err(seed_failed)?;
            }
            transaction
                .execute(
                    "INSERT INTO content_seeds (version, applied_at) VALUES (?1, ?2)",
                    params![QUESTIONS_SEED_VERSION, now],
                )
                .map_err(seed_failed)?;
            transaction.commit().map_err(seed_failed)
        })
    }
}

fn query_questions(
    connection: &Connection,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Question>> {
    let mut statement = connection.prepare(sql).map_err(read_failed)?;
    let mut rows = statement.query(parameters).map_err(read_failed)?;
    let mut questions = Vec::new();
    while let Some(row) = rows.next().map_err(read_failed)? {
        questions.push(question_from_row(row)?);
    }
    Ok(questions)
}

fn find_question(connection: &Connection, id: &str) -> Result<Option<Question>> {
    let parameters: [&dyn rusqlite::ToSql; 1] = [&id];
    let mut questions = query_questions(
        connection,
        "SELECT id, text, category, competencies_json, criteria_json,
                version, created_at, updated_at
         FROM questions
         WHERE id = ?1",
        &parameters,
    )?;
    Ok(questions.pop())
}

fn question_from_row(row: &Row<'_>) -> Result<Question> {
    let category: String = row.get("category").map_err(read_failed)?;
    let category = parse_category(&category)
        .ok_or_else(|| read_corrupt(&format!("categoria desconhecida «{category}»")))?;
    let version: i64 = row.get("version").map_err(read_failed)?;
    let version = Version::try_from(version)
        .map_err(|_| read_corrupt(&format!("versão inválida «{version}»")))?;

    Ok(Question {
        id: row.get("id").map_err(read_failed)?,
        text: row.get("text").map_err(read_failed)?,
        category,
        competencies: json_list(
            &row.get::<_, String>("competencies_json")
                .map_err(read_failed)?,
            "competências",
        )?,
        criteria: json_list(
            &row.get::<_, String>("criteria_json").map_err(read_failed)?,
            "critérios",
        )?,
        version,
        created_at: row.get("created_at").map_err(read_failed)?,
        updated_at: row.get("updated_at").map_err(read_failed)?,
    })
}

pub(super) fn parse_category(value: &str) -> Option<Category> {
    Category::ALL
        .into_iter()
        .find(|category| category.as_str() == value)
}

pub(super) fn json_array(values: &[String]) -> Result<String> {
    serde_json::to_string(values).map_err(|error| {
        AppError::new(
            error_code::STORAGE_WRITE_FAILED,
            format!("Não foi possível guardar os campos da pergunta: {error}"),
            false,
        )
    })
}

pub(super) fn json_list(value: &str, what: &str) -> Result<Vec<String>> {
    serde_json::from_str(value)
        .map_err(|error| read_corrupt(&format!("as {what} guardadas não são válidas ({error})")))
}

fn load_seed() -> Result<Vec<SeedQuestion>> {
    parse_seed(SEED_V1_JSON)
}

fn parse_seed(json: &str) -> Result<Vec<SeedQuestion>> {
    let file: SeedFile = serde_json::from_str(json)
        .map_err(|error| seed_invalid(format!("o JSON não é válido ({error})")))?;

    if file.version != QUESTIONS_SEED_VERSION {
        return Err(seed_invalid(format!(
            "a versão declarada «{}» não corresponde a «{QUESTIONS_SEED_VERSION}»",
            file.version
        )));
    }

    let mut ids = HashSet::with_capacity(file.questions.len());
    let mut questions = Vec::with_capacity(file.questions.len());
    for seed in file.questions {
        if Uuid::parse_str(&seed.id).is_err() {
            return Err(seed_invalid(format!(
                "o identificador «{}» não é um UUID válido",
                seed.id
            )));
        }
        if !ids.insert(seed.id.clone()) {
            return Err(seed_invalid(format!(
                "o identificador «{}» está repetido",
                seed.id
            )));
        }
        let question = validate_question(QuestionInput {
            text: seed.text,
            category: seed.category,
            competencies: seed.competencies,
            criteria: seed.criteria,
        })
        .map_err(|error| {
            seed_invalid(format!(
                "a pergunta «{}» é inválida: {}",
                seed.id, error.message
            ))
        })?;
        questions.push(SeedQuestion {
            id: seed.id,
            text: question.text,
            category: question.category,
            competencies: question.competencies,
            criteria: question.criteria,
        });
    }

    if questions.len() != SEED_QUESTION_COUNT {
        return Err(seed_invalid(format!(
            "o banco inicial tem {} perguntas em vez de {SEED_QUESTION_COUNT}",
            questions.len()
        )));
    }
    for (category, expected) in SEED_DISTRIBUTION {
        let found = questions
            .iter()
            .filter(|question| question.category == category)
            .count();
        if found != expected {
            return Err(seed_invalid(format!(
                "a categoria «{}» tem {found} perguntas em vez de {expected}",
                category.as_str()
            )));
        }
    }

    Ok(questions)
}

/// Instante atual em UTC e RFC3339, com milissegundos.
///
/// A fase 1 não traz uma biblioteca de datas; esta é a mesma conversão civil
/// privada de `service::utc_now`, que este módulo não pode reutilizar sem
/// depender do serviço.
pub(crate) fn utc_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Data civil a partir do número de dias desde 1970-01-01 (algoritmo de Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler o banco de perguntas: {error}"),
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar a pergunta; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

fn seed_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!(
            "Não foi possível preparar o banco inicial de perguntas; \
             nenhuma alteração foi guardada: {error}"
        ),
        true,
    )
}

fn seed_invalid(detail: String) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("O banco inicial de perguntas está inválido e não foi aplicado: {detail}"),
        false,
    )
}

fn read_corrupt(detail: &str) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler o banco de perguntas: {detail}."),
        false,
    )
}

fn stale_edit() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A pergunta foi alterada entretanto. Recarregue o banco de perguntas antes de voltar a guardar.",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_entry(id: &str, text: &str) -> String {
        format!(
            r#"{{"id":"{id}","text":"{text}","category":"problem_solving","competencies":["Análise"],"criteria":["Explicar o desfecho."]}}"#
        )
    }

    fn seed_file(questions: &[String]) -> String {
        format!(
            r#"{{"version":"{QUESTIONS_SEED_VERSION}","questions":[{}]}}"#,
            questions.join(",")
        )
    }

    #[test]
    fn embedded_seed_is_valid_and_unpadded() {
        let questions = load_seed().expect("o seed incluído no binário deve ser válido");
        assert_eq!(questions.len(), SEED_QUESTION_COUNT);
        for (category, expected) in SEED_DISTRIBUTION {
            let found = questions
                .iter()
                .filter(|question| question.category == category)
                .count();
            assert_eq!(found, expected, "distribuição de {}", category.as_str());
        }

        // O ficheiro em si não pode trazer espaços exteriores: o seed guarda o
        // texto da spec tal como está, e só a validação apara as pontas.
        let file: SeedFile = serde_json::from_str(SEED_V1_JSON).expect("JSON do seed");
        for question in &file.questions {
            assert_eq!(question.text, question.text.trim());
            for value in question.competencies.iter().chain(&question.criteria) {
                assert_eq!(value, value.trim());
            }
        }
    }

    #[test]
    fn seed_load_rejects_invalid_content() {
        let wrong_version = parse_seed(r#"{"version":"questions-v2","questions":[]}"#)
            .expect_err("uma versão diferente é recusada");
        assert_eq!(wrong_version.code, error_code::DATABASE_UNAVAILABLE);
        assert!(wrong_version.message.contains("questions-v1"));

        let repeated = seed_file(&[
            seed_entry("1612f4b0-d437-43ec-b898-bb86ec18bf53", "First question."),
            seed_entry("1612f4b0-d437-43ec-b898-bb86ec18bf53", "Second question."),
        ]);
        assert!(parse_seed(&repeated)
            .expect_err("um identificador repetido é recusado")
            .message
            .contains("repetido"));

        let not_a_uuid = seed_file(&[seed_entry("pergunta-1", "First question.")]);
        assert!(parse_seed(&not_a_uuid)
            .expect_err("um identificador que não é UUID é recusado")
            .message
            .contains("UUID"));

        let blank = seed_file(&[seed_entry("1612f4b0-d437-43ec-b898-bb86ec18bf53", "   ")]);
        assert!(parse_seed(&blank)
            .expect_err("um texto em branco é recusado")
            .message
            .contains("inválida"));
    }
}
