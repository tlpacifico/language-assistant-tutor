//! Banco de perguntas: seed único, cobertura das seis categorias, validação
//! dos campos e edição otimista (secção 4 da spec e tarefa 2 do plano).
//!
//! Os testes usam diretórios temporários reais e só a API pública de `Database`
//! e dos contratos da fase 2. O conteúdo inglês é comparado com a tabela da
//! spec, que é a única fonte dessas frases.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use language_assistant_tutor::db::{Database, MIGRATION_V1, QUESTIONS_SEED_VERSION};
use language_assistant_tutor::domain::{error_code, AppError};
use language_assistant_tutor::interview::{Category, QuestionInput};

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

fn input(text: &str, category: Category, competency: &str, criterion: &str) -> QuestionInput {
    QuestionInput {
        text: text.to_owned(),
        category,
        competencies: vec![competency.to_owned()],
        criteria: vec![criterion.to_owned()],
    }
}

/// Textos, categorias, competência e critério específico, pela ordem da spec.
fn expected_bank() -> Vec<(&'static str, Category, &'static str, &'static str)> {
    use Category::*;
    vec![
        (
            "Tell me about a time you disagreed with a colleague.",
            ConflictCollaboration,
            "Gestão de conflitos",
            "Explicar a divergência, a ação própria e o desfecho.",
        ),
        (
            "Describe a time you helped a team work together more effectively.",
            ConflictCollaboration,
            "Colaboração",
            "Identificar o obstáculo e a contribuição individual.",
        ),
        (
            "Tell me about a time you had to explain a technical issue to someone without a technical background.",
            ConflictCollaboration,
            "Comunicação",
            "Explicar a adaptação da mensagem e como verificou a compreensão.",
        ),
        (
            "Describe a time you worked with someone whose approach was different from yours.",
            ConflictCollaboration,
            "Colaboração",
            "Explicar como coordenaram o trabalho e o que aconteceu.",
        ),
        (
            "Tell me about a time you took initiative without being asked.",
            LeadershipInitiative,
            "Iniciativa",
            "Distinguir a decisão própria e o efeito ou aprendizagem.",
        ),
        (
            "Describe a time you helped someone develop a skill.",
            LeadershipInitiative,
            "Liderança",
            "Identificar a necessidade, o apoio prestado e o resultado observado.",
        ),
        (
            "Tell me about a time you influenced a decision without formal authority.",
            LeadershipInitiative,
            "Influência",
            "Explicar os argumentos e a contribuição para a decisão.",
        ),
        (
            "Tell me about a time you made a mistake at work.",
            FailureLearning,
            "Responsabilidade",
            "Reconhecer o papel próprio, a reação e a aprendizagem.",
        ),
        (
            "Describe a time you received feedback that was difficult to hear.",
            FailureLearning,
            "Aprendizagem",
            "Explicar a reação e como usou ou ponderou o feedback.",
        ),
        (
            "Tell me about a time an approach you tried did not work.",
            FailureLearning,
            "Reflexão",
            "Explicar o que falhou e a mudança ou aprendizagem seguinte.",
        ),
        (
            "Tell me about a time you solved a difficult problem at work.",
            ProblemSolving,
            "Resolução de problemas",
            "Explicar o problema, o raciocínio, as ações próprias e o desfecho.",
        ),
        (
            "Describe a time you investigated a problem with incomplete information.",
            ProblemSolving,
            "Análise",
            "Explicar como recolheu informação e lidou com a incerteza.",
        ),
        (
            "Tell me about a time you had to choose between different solutions.",
            ProblemSolving,
            "Decisão",
            "Explicitar alternativas, compromisso e consequência.",
        ),
        (
            "Describe a time you improved a process that was causing problems.",
            ProblemSolving,
            "Melhoria de processos",
            "Distinguir o problema inicial, a mudança e o efeito observado.",
        ),
        (
            "Tell me about a time you had competing deadlines.",
            PrioritiesDeadlines,
            "Priorização",
            "Explicar critérios de prioridade, comunicação e desfecho.",
        ),
        (
            "Describe a time you realised you might miss a deadline.",
            PrioritiesDeadlines,
            "Gestão de expectativas",
            "Explicar quando identificou o risco e como atuou.",
        ),
        (
            "Tell me about a time you had to reduce the scope of a task.",
            PrioritiesDeadlines,
            "Gestão de âmbito",
            "Justificar a escolha e as consequências para o objetivo.",
        ),
        (
            "Tell me about a time your priorities changed unexpectedly.",
            ChangeAdaptation,
            "Adaptação",
            "Explicar a mudança e como reorganizou o trabalho.",
        ),
        (
            "Describe a time you had to learn a new tool or process quickly.",
            ChangeAdaptation,
            "Aprendizagem",
            "Explicar a necessidade, a estratégia e o resultado.",
        ),
        (
            "Tell me about a time you worked through a major change in your team.",
            ChangeAdaptation,
            "Gestão da mudança",
            "Descrever a mudança, a ação individual e a aprendizagem.",
        ),
    ]
}

fn seed_markers(database: &Database) -> Vec<String> {
    database
        .with_connection(|connection| {
            let mut statement = connection
                .prepare("SELECT version FROM content_seeds ORDER BY version")
                .map_err(to_app_error)?;
            let markers = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(to_app_error)?
                .collect::<rusqlite::Result<Vec<String>>>()
                .map_err(to_app_error)?;
            Ok(markers)
        })
        .expect("ler os marcadores de conteúdo aplicado")
}

#[test]
fn seed_is_once_and_preserves_edits() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);

    let seeded = database
        .list_questions(None)
        .expect("listar o banco inicial");
    assert_eq!(
        seeded.len(),
        20,
        "o banco inicial tem exatamente 20 perguntas"
    );
    assert_eq!(
        seed_markers(&database),
        vec![QUESTIONS_SEED_VERSION.to_owned()],
        "a versão do seed é marcada uma vez"
    );

    let edited_source = seeded[0].clone();
    let edited = database
        .update_question(
            &edited_source.id,
            edited_source.version,
            input(
                "Tell me about a time you disagreed with a teammate.",
                Category::ConflictCollaboration,
                "Gestão de conflitos",
                "Explicar a divergência, a ação própria e o desfecho.",
            ),
        )
        .expect("editar uma pergunta do banco");
    assert_eq!(edited.version, 2, "a edição acrescenta uma versão");
    assert_eq!(
        edited.text, "Tell me about a time you disagreed with a teammate.",
        "o texto editado fica guardado"
    );

    // Reabrir a aplicação não repõe o banco nem duplica linhas.
    drop(database);
    let reopened = open(&path);
    let after_reopen = reopened
        .list_questions(None)
        .expect("listar o banco depois de reabrir");
    assert_eq!(after_reopen.len(), 20, "reabrir não duplica perguntas");
    assert_eq!(
        seed_markers(&reopened),
        vec![QUESTIONS_SEED_VERSION.to_owned()],
        "a marca do seed continua única"
    );

    let preserved = after_reopen
        .iter()
        .find(|question| question.id == edited_source.id)
        .expect("a pergunta editada continua no banco");
    assert_eq!(
        preserved.text, "Tell me about a time you disagreed with a teammate.",
        "reabrir não repõe o texto original sobre a edição"
    );
    assert_eq!(preserved.version, 2, "a versão editada é preservada");

    // Repetir o seed explicitamente também não escreve nada.
    reopened
        .ensure_question_seed()
        .expect("repetir o seed deve ser inofensivo");
    assert_eq!(
        reopened.list_questions(None).expect("listar").len(),
        20,
        "repetir o seed não insere perguntas outra vez"
    );
    let still_preserved = reopened
        .list_questions(None)
        .expect("listar")
        .into_iter()
        .find(|question| question.id == edited_source.id)
        .expect("a pergunta editada continua no banco");
    assert_eq!(still_preserved.text, preserved.text);
    assert_eq!(still_preserved.version, 2);
    assert_eq!(
        seed_markers(&reopened),
        vec![QUESTIONS_SEED_VERSION.to_owned()],
        "repetir o seed não escreve outra marca"
    );

    // Os identificadores são estáveis entre aberturas.
    drop(reopened);
    let third = open(&path);
    let ids: Vec<String> = third
        .list_questions(None)
        .expect("listar")
        .into_iter()
        .map(|question| question.id)
        .collect();
    let reopened_ids: Vec<String> = after_reopen
        .iter()
        .map(|question| question.id.clone())
        .collect();
    assert_eq!(
        ids, reopened_ids,
        "os identificadores não mudam entre aberturas"
    );
}

#[test]
fn bank_covers_six_categories() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);

    let expected = expected_bank();
    assert_eq!(expected.len(), 20, "a spec define 20 perguntas");

    let all = database.list_questions(None).expect("listar todas");
    assert_eq!(all.len(), 20, "a distribuição total é 20");

    // O seed mantém a ordem da tabela da spec na primeira abertura.
    let actual: Vec<(&str, Category, &str, &str)> = all
        .iter()
        .map(|question| {
            (
                question.text.as_str(),
                question.category,
                question.competencies[0].as_str(),
                question.criteria[0].as_str(),
            )
        })
        .collect();
    assert_eq!(
        actual, expected,
        "texto, categoria, competência e critério específico seguem a spec"
    );

    // Distribuição 4/3/3/4/3/3 pelas seis categorias.
    let distribution = [
        (Category::ConflictCollaboration, 4),
        (Category::LeadershipInitiative, 3),
        (Category::FailureLearning, 3),
        (Category::ProblemSolving, 4),
        (Category::PrioritiesDeadlines, 3),
        (Category::ChangeAdaptation, 3),
    ];
    assert_eq!(
        distribution.iter().map(|(_, count)| count).sum::<usize>(),
        20
    );
    for (category, count) in distribution {
        let filtered = database
            .list_questions(Some(category))
            .expect("listar por categoria");
        assert_eq!(
            filtered.len(),
            count,
            "a categoria {} deve ter {count} perguntas",
            category.as_str()
        );
        assert!(
            filtered
                .iter()
                .all(|question| question.category == category),
            "o filtro da categoria {} não devolve outras categorias",
            category.as_str()
        );
        assert!(
            filtered.iter().all(|question| {
                !question.competencies.is_empty() && !question.criteria.is_empty()
            }),
            "todas as perguntas têm competência e critério"
        );
    }

    // Identificadores únicos e UUID válidos.
    let mut ids: Vec<&str> = all.iter().map(|question| question.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        20,
        "as 20 perguntas têm identificadores distintos"
    );
}

#[test]
fn question_trims_only_outer_whitespace() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);

    let created = database
        .create_question(QuestionInput {
            text: "  Describe a time\n  the team   disagreed, calmly.  ".to_owned(),
            category: Category::ProblemSolving,
            competencies: vec!["  Análise  ".to_owned()],
            criteria: vec!["  Explicar o desfecho.  ".to_owned()],
        })
        .expect("criar uma pergunta com espaços exteriores");

    assert_eq!(
        created.text, "Describe a time\n  the team   disagreed, calmly.",
        "só os espaços exteriores são ignorados; o interior fica intacto"
    );
    assert_eq!(
        created.competencies,
        vec!["Análise".to_owned()],
        "a competência é aparada nas pontas"
    );
    assert_eq!(
        created.criteria,
        vec!["Explicar o desfecho.".to_owned()],
        "o critério é aparado nas pontas"
    );
    assert_eq!(created.version, 1, "uma pergunta nova começa na versão 1");
    assert_eq!(created.created_at, created.updated_at);

    // A mesma regra aplica-se à edição, e o interior não é reescrito.
    let edited = database
        .update_question(
            &created.id,
            created.version,
            QuestionInput {
                text: "  Describe a time\n  the team   disagreed, calmly.  ".to_owned(),
                category: Category::ProblemSolving,
                competencies: vec!["  Análise  ".to_owned()],
                criteria: vec!["  Explicar o desfecho.  ".to_owned()],
            },
        )
        .expect("editar com espaços exteriores");
    assert_eq!(edited.text, created.text);
    assert_eq!(edited.competencies, created.competencies);
    assert_eq!(edited.criteria, created.criteria);

    // O texto guardado é o aparado, também depois de reabrir.
    drop(database);
    let reopened = open(&path);
    let stored = reopened
        .list_questions(None)
        .expect("listar")
        .into_iter()
        .find(|question| question.id == created.id)
        .expect("a pergunta criada continua no banco");
    assert_eq!(stored.text, created.text);
    assert_eq!(stored.competencies, created.competencies);
    assert_eq!(stored.criteria, created.criteria);
}

#[test]
fn blank_fields_are_rejected() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);

    let before = database.list_questions(None).expect("listar antes").len();

    let cases = vec![
        (
            "texto vazio",
            input("   ", Category::ProblemSolving, "Análise", "Critério"),
        ),
        (
            "texto em branco",
            input("\n\t ", Category::ProblemSolving, "Análise", "Critério"),
        ),
        (
            "sem competências",
            QuestionInput {
                text: "Describe a time you tested a blank bank.".to_owned(),
                category: Category::ProblemSolving,
                competencies: Vec::new(),
                criteria: vec!["Explicar o desfecho.".to_owned()],
            },
        ),
        (
            "competência em branco",
            input(
                "Describe a time you tested a blank bank.",
                Category::ProblemSolving,
                "   ",
                "Explicar o desfecho.",
            ),
        ),
        (
            "sem critérios",
            QuestionInput {
                text: "Describe a time you tested a blank bank.".to_owned(),
                category: Category::ProblemSolving,
                competencies: vec!["Análise".to_owned()],
                criteria: Vec::new(),
            },
        ),
        (
            "critério em branco",
            input(
                "Describe a time you tested a blank bank.",
                Category::ProblemSolving,
                "Análise",
                "  \n ",
            ),
        ),
    ];

    for (case, invalid) in cases {
        let error = database
            .create_question(invalid)
            .expect_err("campos obrigatórios em branco são recusados");
        assert_eq!(
            error.code,
            error_code::INVALID_INPUT,
            "{case}: esperava INVALID_INPUT, recebi {error:?}"
        );
        assert!(!error.retryable, "{case}: o erro não é repetível");
        assert!(
            error.message.contains("pergunta")
                || error.message.contains("competência")
                || error.message.contains("critério"),
            "{case}: a mensagem deve orientar o utilizador, recebi {error:?}"
        );
    }

    assert_eq!(
        database.list_questions(None).expect("listar depois").len(),
        before,
        "nenhuma pergunta inválida foi guardada"
    );

    // A edição aplica a mesma validação e não altera a versão.
    let target = database.list_questions(None).expect("listar")[0].clone();
    let error = database
        .update_question(
            &target.id,
            target.version,
            input("   ", target.category, "Análise", "Critério"),
        )
        .expect_err("uma edição em branco é recusada");
    assert_eq!(error.code, error_code::INVALID_INPUT);
    let unchanged = database
        .list_questions(None)
        .expect("listar")
        .into_iter()
        .find(|question| question.id == target.id)
        .expect("a pergunta continua no banco");
    assert_eq!(unchanged, target, "a edição recusada não altera a pergunta");
}

#[test]
fn stale_edit_does_not_overwrite() {
    let (_directory, path) = temp_database_path();
    let database = open(&path);

    let target = database.list_questions(None).expect("listar")[3].clone();
    let first_text = "Describe a time you worked with someone whose approach differed from yours.";
    // Os instantes têm milissegundos; a pausa torna a mudança de updatedAt
    // observável em vez de depender da resolução do relógio.
    thread::sleep(Duration::from_millis(20));
    let first = database
        .update_question(
            &target.id,
            target.version,
            input(
                first_text,
                target.category,
                &target.competencies[0],
                &target.criteria[0],
            ),
        )
        .expect("a primeira edição usa a versão atual");
    assert_eq!(first.version, target.version + 1);
    assert!(
        first.updated_at > target.updated_at,
        "uma edição bem-sucedida atualiza updatedAt: {:?} -> {:?}",
        target.updated_at,
        first.updated_at
    );

    let stale = database
        .update_question(
            &target.id,
            target.version,
            input(
                "Describe a time you worked with someone whose approach was entirely different from yours.",
                target.category,
                &target.competencies[0],
                &target.criteria[0],
            ),
        )
        .expect_err("uma versão antiga é recusada");
    assert_eq!(
        stale.code,
        error_code::STALE_SOURCE,
        "esperava STALE_SOURCE, recebi {stale:?}"
    );
    assert!(!stale.retryable, "recarregar o banco resolve o conflito");

    let stored = database
        .list_questions(None)
        .expect("listar")
        .into_iter()
        .find(|question| question.id == target.id)
        .expect("a pergunta continua no banco");
    assert_eq!(
        stored.text, first_text,
        "a edição antiga não substitui o texto mais recente"
    );
    assert_eq!(stored.version, first.version, "a versão não recua");

    // Um identificador desconhecido também é uma origem desatualizada.
    let unknown = database
        .update_question(
            "identificador-inexistente",
            1,
            input(
                "Describe a time you tested an unknown identifier.",
                Category::ProblemSolving,
                "Análise",
                "Explicar o desfecho.",
            ),
        )
        .expect_err("uma pergunta inexistente é recusada");
    assert_eq!(unknown.code, error_code::STALE_SOURCE);
    assert_eq!(
        database.list_questions(None).expect("listar").len(),
        20,
        "a edição de uma pergunta inexistente não cria linhas"
    );
}

#[test]
fn incomplete_seed_rolls_back_without_marker() {
    let (_directory, path) = temp_database_path();

    // Só o esquema (sem seed) e uma pergunta manual que colide com um dos
    // identificadores fixos do banco inicial.
    let database = Database::open_with_schema(path.clone(), MIGRATION_V1)
        .expect("aplicar só o esquema da fase 2");
    database
        .with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO questions
                       (id, text, category, competencies_json, criteria_json, version, created_at, updated_at)
                     VALUES ('1612f4b0-d437-43ec-b898-bb86ec18bf53', 'Pergunta manual.',
                             'problem_solving', '[\"Análise\"]', '[\"Explicar o desfecho.\"]', 1,
                             '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z')",
                    [],
                )
                .map_err(to_app_error)?;
            Ok(())
        })
        .expect("inserir a pergunta manual");

    let error = database
        .ensure_question_seed()
        .expect_err("o seed incompleto tem de falhar e reverter");
    assert_eq!(
        error.code,
        error_code::STORAGE_WRITE_FAILED,
        "esperava STORAGE_WRITE_FAILED, recebi {error:?}"
    );
    assert!(error.retryable, "uma colisão de escrita é recuperável");

    let count = database
        .with_connection(|connection| {
            connection
                .query_row("SELECT count(*) FROM questions", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(to_app_error)
        })
        .expect("contar as perguntas");
    assert_eq!(
        count, 1,
        "as 20 inserções do seed falhado são revertidas; fica só a manual"
    );
    assert!(
        seed_markers(&database).is_empty(),
        "sem marca de versão aplicada: '{}' não pode constar em content_seeds",
        QUESTIONS_SEED_VERSION
    );

    // Resolvida a colisão, o seed volta a poder aplicar-se por inteiro.
    database
        .with_connection(|connection| {
            connection
                .execute("DELETE FROM questions", [])
                .map_err(to_app_error)?;
            Ok(())
        })
        .expect("limpar a pergunta manual");
    database
        .ensure_question_seed()
        .expect("o seed completo aplica-se depois do rollback");
    assert_eq!(
        database.list_questions(None).expect("listar").len(),
        20,
        "o seed aplica-se por inteiro depois de o conflito desaparecer"
    );
    assert_eq!(
        seed_markers(&database),
        vec![QUESTIONS_SEED_VERSION.to_owned()]
    );
}
