//! Sessões guiadas: criação atómica, sorteio sem repetição, cópias imutáveis e
//! consulta (secção 5 da spec e tarefa 3 do plano).
//!
//! Os testes usam diretórios temporários reais e só a API pública de `Database`
//! sobre o banco inicial de 20 perguntas. O sorteio não é verificado com
//! expectativas estatísticas: verificam-se o conjunto, a unicidade, as posições
//! guardadas e que a ordem guardada é exatamente a que `load_session` devolve.

use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::thread;

use rusqlite::params;

use language_assistant_tutor::db::{Database, STAR_CRITERIA};
use language_assistant_tutor::domain::{error_code, AppError};
use language_assistant_tutor::interview::{
    Category, QuestionInput, SessionDetail, SessionMode, SessionStatus, StartSession,
};

fn temp_database_path() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("criar o diretório temporário");
    let path = directory.path().join("tutor.sqlite3");
    (directory, path)
}

fn to_app_error(error: rusqlite::Error) -> AppError {
    AppError::new("TEST_SQLITE", error.to_string(), false)
}

fn open(path: &Path) -> Database {
    Database::open(path.to_path_buf()).expect("abrir a base de teste")
}

fn request(request_id: &str, category: Option<Category>, count: u32) -> StartSession {
    StartSession {
        request_id: request_id.to_owned(),
        category,
        count,
    }
}

fn sqlite_count(database: &Database, table: &str) -> i64 {
    database
        .with_connection(|connection| {
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(to_app_error)
        })
        .expect("contar as linhas da tabela")
}

/// Critérios STAR comuns primeiro, critérios específicos da pergunta no fim.
fn assert_star_then_specific(criteria: &[String], specific: &[String]) {
    assert_eq!(
        criteria.len(),
        STAR_CRITERIA.len() + specific.len(),
        "a cópia deve juntar os critérios comuns aos específicos: {criteria:?}"
    );
    for (index, common) in STAR_CRITERIA.iter().enumerate() {
        assert_eq!(
            criteria[index], *common,
            "o critério STAR {index} deve vir primeiro"
        );
    }
    assert_eq!(
        &criteria[STAR_CRITERIA.len()..],
        specific,
        "os critérios específicos são copiados no fim"
    );
}

fn unique<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut values: Vec<&str> = values.into_iter().collect();
    values.sort_unstable();
    values.dedup();
    values
}

#[test]
fn starts_one_three_and_available_count() {
    // Uma pergunta: a configuração mínima.
    {
        let (_directory, path) = temp_database_path();
        let database = open(&path);
        let bank = database.list_questions(None).expect("listar o banco");

        let single = database
            .start_session(request("pedido-um", None, 1))
            .expect("iniciar uma entrevista com uma pergunta");

        assert_eq!(single.summary.count, 1, "a sessão guarda a quantidade");
        assert_eq!(single.summary.confirmed_count, 0, "nada está confirmado");
        assert_eq!(single.summary.position, 1, "a sessão começa na posição 1");
        assert_eq!(single.summary.status, SessionStatus::InProgress);
        assert_eq!(single.summary.mode, SessionMode::LocalV2);
        assert_eq!(
            single.summary.version, 1,
            "uma sessão nova começa na versão 1"
        );
        assert!(single.summary.completed_at.is_none());
        assert!(single.summary.category.is_none(), "todas as categorias");

        assert_eq!(single.questions.len(), 1);
        let detail = &single.questions[0];
        assert!(detail.recordings.is_empty(), "ainda não há gravações");
        assert!(detail.confirmation.is_none());
        let question = &detail.question;
        assert_eq!(question.position, 1);
        assert_eq!(question.session_id, single.summary.id);
        assert!(question.selected_recording_id.is_none());
        assert_eq!(question.selection_version, 0);

        let source = bank
            .iter()
            .find(|candidate| candidate.id == question.source_question_id)
            .expect("a cópia aponta para uma pergunta do banco");
        assert_eq!(question.text, source.text);
        assert_eq!(question.category, source.category);
        assert_eq!(question.competencies, source.competencies);
        assert_eq!(question.source_version, source.version);
        assert_star_then_specific(&question.criteria, &source.criteria);

        // A consulta devolve o mesmo estado, sem escrever.
        let loaded = database
            .load_session(&single.summary.id)
            .expect("carregar a sessão criada");
        assert_eq!(loaded, single, "load_session devolve a seleção guardada");
        assert_eq!(
            database
                .active_session()
                .expect("ler a sessão em curso")
                .as_ref(),
            Some(&single)
        );
        let summaries = database.list_sessions().expect("listar as sessões");
        assert_eq!(summaries, vec![single.summary.clone()]);
        assert_eq!(
            database.list_sessions().expect("reler as sessões"),
            summaries,
            "listar é apenas leitura"
        );
        assert_eq!(
            database.load_session(&single.summary.id).expect("reler"),
            single
        );
    }

    // Três perguntas: posições 1..N, sem repetição, critérios completos.
    {
        let (_directory, path) = temp_database_path();
        let database = open(&path);
        let bank = database.list_questions(None).expect("listar o banco");

        let started = database
            .start_session(request("pedido-três", None, 3))
            .expect("iniciar uma entrevista com três perguntas");
        assert_eq!(started.summary.count, 3);
        assert_eq!(started.summary.position, 1);
        assert_eq!(started.questions.len(), 3);

        let positions: Vec<u32> = started
            .questions
            .iter()
            .map(|detail| detail.question.position)
            .collect();
        assert_eq!(
            positions,
            vec![1, 2, 3],
            "posições 1..N pela ordem guardada"
        );

        let sources: Vec<&str> = started
            .questions
            .iter()
            .map(|detail| detail.question.source_question_id.as_str())
            .collect();
        assert_eq!(unique(sources).len(), 3, "as três perguntas não se repetem");

        for detail in &started.questions {
            let source = bank
                .iter()
                .find(|question| question.id == detail.question.source_question_id)
                .expect("a cópia aponta para uma pergunta do banco");
            assert_eq!(detail.question.text, source.text);
            assert_eq!(detail.question.category, source.category);
            assert_eq!(detail.question.competencies, source.competencies);
            assert_eq!(detail.question.source_version, source.version);
            assert_star_then_specific(&detail.question.criteria, &source.criteria);
            assert!(detail.recordings.is_empty());
            assert!(detail.confirmation.is_none());
        }

        // A ordem guardada é exatamente a que a consulta devolve.
        let loaded = database
            .load_session(&started.summary.id)
            .expect("carregar a sessão criada");
        assert_eq!(loaded, started, "load_session preserva a ordem guardada");
        assert_eq!(
            database
                .active_session()
                .expect("ler a sessão em curso")
                .as_ref(),
            Some(&started)
        );
    }

    // Todas as elegíveis (o limite é o total do banco, não uma redução).
    {
        let (_directory, path) = temp_database_path();
        let database = open(&path);
        let available = database.list_questions(None).expect("listar o banco").len();
        assert_eq!(available, 20, "o banco inicial tem vinte perguntas");

        let started = database
            .start_session(request("pedido-todas", None, available as u32))
            .expect("iniciar com todas as perguntas elegíveis");
        assert_eq!(started.summary.count, available as u32);
        assert_eq!(started.questions.len(), available);

        let positions: Vec<u32> = started
            .questions
            .iter()
            .map(|detail| detail.question.position)
            .collect();
        assert_eq!(positions, (1..=available as u32).collect::<Vec<u32>>());

        let sources: Vec<&str> = started
            .questions
            .iter()
            .map(|detail| detail.question.source_question_id.as_str())
            .collect();
        assert_eq!(
            unique(sources).len(),
            available,
            "todas as perguntas elegíveis entram uma única vez"
        );

        // A categoria escolhida também respeita o total elegível dela.
        drop(database);
        let (_second_directory, second_path) = temp_database_path();
        let second = open(&second_path);
        let category_available = second
            .list_questions(Some(Category::ConflictCollaboration))
            .expect("listar a categoria")
            .len();
        assert_eq!(category_available, 4);
        let filtered = second
            .start_session(request(
                "pedido-categoria",
                Some(Category::ConflictCollaboration),
                category_available as u32,
            ))
            .expect("iniciar com todas as elegíveis da categoria");
        assert_eq!(filtered.questions.len(), category_available);
        assert!(
            filtered
                .questions
                .iter()
                .all(|detail| detail.question.category == Category::ConflictCollaboration),
            "a seleção filtrada não mistura categorias"
        );
        assert_eq!(
            filtered.summary.category,
            Some(Category::ConflictCollaboration)
        );
    }
}

#[test]
fn insufficient_category_creates_nothing() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);
    let questions_before = sqlite_count(&database, "questions");

    // Quantidade zero: a mensagem indica as perguntas disponíveis.
    let zero = database
        .start_session(request("pedido-zero", None, 0))
        .expect_err("zero perguntas não é aceite");
    assert_eq!(
        zero.code,
        error_code::INSUFFICIENT_QUESTIONS,
        "esperava INSUFFICIENT_QUESTIONS, recebi {zero:?}"
    );
    assert!(
        !zero.retryable,
        "repetir sem ajustar a quantidade não resolve"
    );
    assert!(
        zero.message.contains("20"),
        "a mensagem deve indicar as perguntas disponíveis: {}",
        zero.message
    );

    // Mais do que as elegíveis na categoria: sem redução silenciosa.
    let category = Category::FailureLearning;
    let available = database
        .list_questions(Some(category))
        .expect("listar a categoria")
        .len();
    assert_eq!(
        available, 3,
        "a categoria tem três perguntas no banco inicial"
    );
    let excess = database
        .start_session(request("pedido-excesso", Some(category), 4))
        .expect_err("mais do que as elegíveis não é aceite");
    assert_eq!(
        excess.code,
        error_code::INSUFFICIENT_QUESTIONS,
        "esperava INSUFFICIENT_QUESTIONS, recebi {excess:?}"
    );
    assert!(!excess.retryable);
    assert!(
        excess.message.contains('3'),
        "a mensagem deve indicar as disponíveis: {}",
        excess.message
    );

    // Nada foi criado: nem sessão, nem perguntas, nem recibo.
    assert_eq!(sqlite_count(&database, "sessions"), 0);
    assert_eq!(sqlite_count(&database, "session_questions"), 0);
    assert_eq!(sqlite_count(&database, "command_receipts"), 0);
    assert_eq!(
        sqlite_count(&database, "questions"),
        questions_before,
        "as recusas não alteram o banco de perguntas"
    );
    assert!(
        database
            .active_session()
            .expect("ler a sessão em curso")
            .is_none(),
        "uma recusa não deixa sessão em curso"
    );
    assert!(database.list_sessions().expect("listar").is_empty());

    // O pedido recusado não deixou recibo: o mesmo requestId pode iniciar
    // depois de o utilizador ajustar a quantidade.
    let started = database
        .start_session(request("pedido-zero", Some(category), available as u32))
        .expect("o pedido ajustado é aceite");
    assert_eq!(started.questions.len(), 3);
    assert_eq!(sqlite_count(&database, "sessions"), 1);
}

#[test]
fn snapshots_survive_bank_edits() {
    let (_directory, path) = temp_database_path();

    let started;
    {
        let database = open(&path);
        started = database
            .start_session(request("pedido-fixo", None, 3))
            .expect("iniciar a entrevista");

        let bank = database.list_questions(None).expect("listar o banco");
        for detail in &started.questions {
            let source = bank
                .iter()
                .find(|question| question.id == detail.question.source_question_id)
                .expect("a pergunta de origem continua no banco");
            let edited = database
                .update_question(
                    &source.id,
                    source.version,
                    QuestionInput {
                        text: format!("Edited question for {}.", source.id),
                        category: Category::ChangeAdaptation,
                        competencies: vec!["Competência editada".to_owned()],
                        criteria: vec!["Critério editado.".to_owned()],
                    },
                )
                .expect("editar a pergunta de origem");
            assert_eq!(edited.version, 2, "a edição acrescenta uma versão");
        }

        // Uma pergunta nova no banco também não entra numa sessão já criada.
        database
            .create_question(QuestionInput {
                text: "Tell me about a time you tested a fixed snapshot.".to_owned(),
                category: Category::ProblemSolving,
                competencies: vec!["Análise".to_owned()],
                criteria: vec!["Explicar o teste.".to_owned()],
            })
            .expect("criar uma pergunta nova");

        let loaded = database
            .load_session(&started.summary.id)
            .expect("recarregar a sessão");
        assert_eq!(
            loaded, started,
            "editar o banco não altera a cópia da sessão"
        );

        let bank_now = database.list_questions(None).expect("listar o banco");
        assert_eq!(
            bank_now.len(),
            21,
            "o banco tem as edições e a pergunta nova"
        );
        for detail in &started.questions {
            let source_now = bank_now
                .iter()
                .find(|question| question.id == detail.question.source_question_id)
                .expect("a origem continua no banco");
            assert_eq!(source_now.version, 2);
            assert_eq!(
                source_now.text,
                format!("Edited question for {}.", source_now.id),
                "o banco de perguntas ficou mesmo editado"
            );
            assert_ne!(
                detail.question.text, source_now.text,
                "o texto editado no banco não entra na cópia da sessão"
            );
        }
    }

    // Reabrir também conserva as cópias imutáveis.
    let reopened = open(&path);
    let loaded = reopened
        .load_session(&started.summary.id)
        .expect("carregar a sessão depois de reabrir");
    assert_eq!(loaded, started, "reabrir não altera as cópias");
    for detail in &loaded.questions {
        assert_eq!(
            detail.question.criteria.len(),
            STAR_CRITERIA.len() + 1,
            "só os critérios STAR e o específico original foram copiados"
        );
        assert_eq!(detail.question.criteria[0], STAR_CRITERIA[0]);
        assert!(
            !detail
                .question
                .competencies
                .iter()
                .any(|value| value == "Competência editada"),
            "a competência editada não entra na cópia: {:?}",
            detail.question.competencies
        );
        assert!(
            !detail
                .question
                .criteria
                .iter()
                .any(|value| value == "Critério editado."),
            "o critério editado não entra na cópia: {:?}",
            detail.question.criteria
        );
        assert!(
            !detail.question.text.starts_with("Edited question for"),
            "o texto editado não entra na cópia: {:?}",
            detail.question.text
        );
    }
    assert_eq!(
        reopened.list_sessions().expect("listar"),
        vec![loaded.summary.clone()]
    );
}

#[test]
fn repeated_start_returns_original_session() {
    let (_directory, path) = temp_database_path();
    let first;
    {
        let database = open(&path);
        let payload = request("pedido-repetido", Some(Category::ProblemSolving), 3);
        first = database
            .start_session(payload.clone())
            .expect("iniciar a entrevista");

        let again = database
            .start_session(payload.clone())
            .expect("repetir o mesmo pedido devolve a sessão original");
        assert_eq!(again, first, "o recibo devolve exatamente a mesma sessão");
        assert_eq!(sqlite_count(&database, "sessions"), 1);
        assert_eq!(sqlite_count(&database, "session_questions"), 3);
        assert_eq!(sqlite_count(&database, "command_receipts"), 1);

        // O mesmo requestId com outra configuração é uma origem desatualizada.
        let changed = database
            .start_session(request(
                "pedido-repetido",
                Some(Category::ProblemSolving),
                1,
            ))
            .expect_err("o mesmo pedido com outra configuração é recusado");
        assert_eq!(
            changed.code,
            error_code::STALE_SOURCE,
            "esperava STALE_SOURCE, recebi {changed:?}"
        );
        assert!(!changed.retryable);
        assert_eq!(sqlite_count(&database, "sessions"), 1);
        assert_eq!(sqlite_count(&database, "session_questions"), 3);
        assert_eq!(
            database.load_session(&first.summary.id).expect("carregar"),
            first
        );

        // Uma sessão concluída continua a ser devolvida pelo seu recibo: o
        // replay nunca cria uma entrevista nova.
        database
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE sessions
                         SET status = 'completed', completed_at = '2026-11-02T10:00:00.000Z'
                         WHERE id = ?1",
                        params![first.summary.id],
                    )
                    .map_err(to_app_error)?;
                Ok(())
            })
            .expect("marcar a sessão como concluída");
        let completed = database
            .start_session(payload)
            .expect("o recibo devolve a sessão concluída");
        assert_eq!(completed.summary.id, first.summary.id);
        assert_eq!(completed.summary.status, SessionStatus::Completed);
        assert_eq!(
            completed.summary.completed_at.as_deref(),
            Some("2026-11-02T10:00:00.000Z")
        );
        assert_eq!(completed.questions, first.questions);
        assert_eq!(sqlite_count(&database, "sessions"), 1);
        assert_eq!(sqlite_count(&database, "command_receipts"), 1);

        // Depois de concluída, uma nova configuração pode iniciar outra sessão.
        let new_session = database
            .start_session(request("pedido-novo", Some(Category::ProblemSolving), 1))
            .expect("uma sessão concluída permite iniciar outra");
        assert_eq!(new_session.summary.status, SessionStatus::InProgress);
        assert_ne!(new_session.summary.id, first.summary.id);
        assert_eq!(sqlite_count(&database, "sessions"), 2);
    }

    // Após reabrir, o recibo continua a devolver a sessão original.
    let reopened = open(&path);
    let replay = reopened
        .start_session(request(
            "pedido-repetido",
            Some(Category::ProblemSolving),
            3,
        ))
        .expect("repetir depois de reabrir");
    assert_eq!(replay.summary.id, first.summary.id);
    assert_eq!(replay.questions, first.questions);
    assert_eq!(
        sqlite_count(&reopened, "sessions"),
        2,
        "o replay não insere outra sessão"
    );
    assert_eq!(
        sqlite_count(&reopened, "command_receipts"),
        2,
        "o recibo original continua único"
    );
}

#[test]
fn only_one_session_can_be_active() {
    let (_directory, path) = temp_database_path();

    // Duas ligações reais à mesma base: o índice parcial é a única garantia
    // entre transações concorrentes.
    let first_database = open(&path);
    let second_database = open(&path);
    let barrier = Barrier::new(2);
    let results = thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            first_database.start_session(request("pedido-a", None, 2))
        });
        let second = scope.spawn(|| {
            barrier.wait();
            second_database.start_session(request("pedido-b", None, 2))
        });
        vec![
            first.join().expect("a primeira ligação termina"),
            second.join().expect("a segunda ligação termina"),
        ]
    });

    let successes: Vec<&SessionDetail> = results
        .iter()
        .filter_map(|result| result.as_ref().ok())
        .collect();
    let failures: Vec<&AppError> = results
        .iter()
        .filter_map(|result| result.as_ref().err())
        .collect();
    assert_eq!(
        successes.len(),
        1,
        "exatamente um pedido cria a sessão: {results:?}"
    );
    assert_eq!(failures.len(), 1, "o outro pedido é recusado: {results:?}");
    let failure = failures[0];
    assert_eq!(
        failure.code,
        error_code::ACTIVE_SESSION_EXISTS,
        "esperava ACTIVE_SESSION_EXISTS, recebi {failure:?}"
    );
    assert!(
        !failure.retryable,
        "repetir cegamente não resolve; é preciso retomar ou terminar"
    );
    assert!(
        failure.message.contains("em curso"),
        "a mensagem deve explicar o conflito: {}",
        failure.message
    );

    let database = open(&path);
    assert_eq!(
        sqlite_count(&database, "sessions"),
        1,
        "o índice único impede uma segunda sessão em curso"
    );
    assert_eq!(sqlite_count(&database, "command_receipts"), 1);
    assert_eq!(
        sqlite_count(&database, "session_questions"),
        2,
        "só a sessão vencedora guardou perguntas"
    );
    let active = database
        .active_session()
        .expect("ler a sessão em curso")
        .expect("a sessão vencedora continua em curso");
    assert_eq!(active.summary.id, successes[0].summary.id);
    assert_eq!(active.questions.len(), 2);

    // Sequencialmente, com outro requestId, a regra mantém-se.
    let later = database
        .start_session(request("pedido-c", None, 2))
        .expect_err("uma segunda sessão em curso é recusada");
    assert_eq!(later.code, error_code::ACTIVE_SESSION_EXISTS);
    assert_eq!(sqlite_count(&database, "sessions"), 1);
}

#[test]
fn failed_second_question_rolls_back_session() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);

    // Erro injetado depois de a primeira pergunta da sessão ser inserida: a
    // transação inteira tem de reverter.
    database
        .with_connection(|connection| {
            connection
                .execute_batch(
                    "CREATE TRIGGER test_fail_second_question
                     BEFORE INSERT ON session_questions
                     WHEN (SELECT count(*) FROM session_questions
                           WHERE session_id = NEW.session_id) >= 1
                     BEGIN
                         SELECT RAISE(ABORT, 'falha injetada depois da primeira pergunta');
                     END;",
                )
                .map_err(to_app_error)
        })
        .expect("instalar a falha injetada");

    let error = database
        .start_session(request("pedido-falhado", None, 3))
        .expect_err("a falha injetada aborta a criação");
    assert_eq!(
        error.code,
        error_code::STORAGE_WRITE_FAILED,
        "esperava STORAGE_WRITE_FAILED, recebi {error:?}"
    );
    assert!(
        error.retryable,
        "repetir depois de resolver a falha é seguro"
    );

    assert_eq!(
        sqlite_count(&database, "sessions"),
        0,
        "a sessão inteira desaparece por rollback"
    );
    assert_eq!(
        sqlite_count(&database, "session_questions"),
        0,
        "a primeira pergunta inserida também é revertida"
    );
    assert_eq!(
        sqlite_count(&database, "command_receipts"),
        0,
        "uma criação falhada não deixa recibo"
    );
    assert!(
        database.active_session().expect("ler").is_none(),
        "não fica sessão em curso depois da falha"
    );
    assert!(database.list_sessions().expect("listar").is_empty());

    // Removida a falha, o mesmo pedido volta a poder criar a sessão inteira.
    database
        .with_connection(|connection| {
            connection
                .execute("DROP TRIGGER test_fail_second_question", [])
                .map_err(to_app_error)?;
            Ok(())
        })
        .expect("remover a falha injetada");
    let started = database
        .start_session(request("pedido-falhado", None, 3))
        .expect("o mesmo pedido funciona depois de a falha desaparecer");
    assert_eq!(started.questions.len(), 3);
    assert_eq!(sqlite_count(&database, "sessions"), 1);
    assert_eq!(sqlite_count(&database, "command_receipts"), 1);
}

#[test]
fn list_sessions_reports_completed_sessions_and_order() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);
    let bank = database.list_questions(None).expect("listar o banco");
    let source = bank[0].clone();

    // Uma sessão concluída de uma execução anterior, com uma pergunta copiada.
    database
        .with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO sessions
                       (id, category, question_count, position, status, mode, version,
                        created_at, completed_at)
                     VALUES ('sessao-antiga', ?1, 1, 1, 'completed', 'local_v2', 3, ?2, ?3)",
                    params![
                        source.category.as_str(),
                        "2026-01-02T09:00:00.000Z",
                        "2026-01-02T09:30:00.000Z"
                    ],
                )
                .map_err(to_app_error)?;
            connection
                .execute(
                    "INSERT INTO session_questions
                       (id, session_id, source_question_id, source_version, position, text,
                        category, competencies_json, criteria_json, selected_recording_id,
                        selection_version)
                     VALUES ('sq-antiga', 'sessao-antiga', ?1, ?2, 1, ?3, ?4, ?5, ?6, NULL, 0)",
                    params![
                        source.id,
                        i64::try_from(source.version).expect("versão de teste"),
                        source.text,
                        source.category.as_str(),
                        serde_json::to_string(&source.competencies).expect("competências"),
                        serde_json::to_string(&source.criteria).expect("critérios")
                    ],
                )
                .map_err(to_app_error)?;
            Ok(())
        })
        .expect("preparar uma sessão concluída");

    let started = database
        .start_session(request("pedido-atual", None, 2))
        .expect("iniciar a sessão atual");

    let summaries = database.list_sessions().expect("listar o histórico");
    assert_eq!(summaries.len(), 2);
    assert_eq!(
        summaries[0], started.summary,
        "a sessão mais recente vem primeiro"
    );

    let old = summaries
        .iter()
        .find(|summary| summary.id == "sessao-antiga")
        .expect("a sessão concluída consta no histórico");
    assert_eq!(old.status, SessionStatus::Completed);
    assert_eq!(
        old.completed_at.as_deref(),
        Some("2026-01-02T09:30:00.000Z")
    );
    assert_eq!(old.version, 3);
    assert_eq!(old.mode, SessionMode::LocalV2);
    assert_eq!(old.category, Some(source.category));
    assert_eq!(old.count, 1);
    assert_eq!(old.position, 1);
    assert_eq!(old.confirmed_count, 0);

    let active = database
        .active_session()
        .expect("ler a sessão em curso")
        .expect("há uma sessão em curso");
    assert_eq!(active.summary.id, started.summary.id);

    let old_detail = database
        .load_session("sessao-antiga")
        .expect("carregar a sessão concluída");
    assert_eq!(old_detail.summary, *old);
    assert_eq!(old_detail.questions.len(), 1);
    assert_eq!(old_detail.questions[0].question.id, "sq-antiga");
    assert_eq!(old_detail.questions[0].question.text, source.text);

    assert_eq!(
        database.list_sessions().expect("reler o histórico"),
        summaries,
        "as leituras não alteram o histórico"
    );
    assert_eq!(
        database.load_session("sessao-antiga").expect("reler"),
        old_detail
    );
}
