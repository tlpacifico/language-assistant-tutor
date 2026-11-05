//! Persistência das conversas sobre artigos: criação, mutações, recibos e fontes.

use language_assistant_tutor::ai::{build_article_body, AiSettingsView, ArticleAiTask};
use language_assistant_tutor::article::{
    ArticleCommand, ArticleCreate, ArticleError, ArticleMutation, ArticleSnapshot, ArticleSpeaker,
    ArticleState, ArticleVersionSource, ARTICLE_NOTICE_VERSION, MAX_ANSWER_BYTES,
    MAX_ARTICLE_BYTES, MAX_TUTOR_REPLY_BYTES,
};
use language_assistant_tutor::db::Database;

const DESTINATION: &str = "https://opencode.ai/inference/go/openai/v1";

fn database() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().expect("diretório temporário");
    let database = Database::open(directory.path().join("tutor.sqlite3")).expect("abrir");
    (directory, database)
}

fn create_input(request_id: &str) -> ArticleCreate {
    ArticleCreate {
        request_id: request_id.into(),
        title: Some("Remote work".into()),
        text: "  Remote work changed collaboration.  ".into(),
    }
}

fn command(snapshot: &ArticleSnapshot, request_id: &str) -> ArticleCommand {
    ArticleCommand {
        request_id: request_id.into(),
        session_id: snapshot.session.id.clone(),
        expected_version: snapshot.session.version,
    }
}

fn authorize() -> ArticleMutation {
    ArticleMutation::Authorize {
        destination: DESTINATION.into(),
        notice_version: ARTICLE_NOTICE_VERSION.into(),
    }
}

#[test]
fn create_keeps_trimmed_source_and_one_active_session() {
    let (_directory, database) = database();
    let created = database.create_article(create_input("c1")).unwrap();
    assert_eq!(created.session.state, ArticleState::Draft);
    assert_eq!(
        created.session.article_text,
        "Remote work changed collaboration."
    );
    assert_eq!(created.session.version, 1);
    assert!(created.session.authorization.is_none());

    // Repetir o mesmo pedido devolve a mesma sessão.
    let again = database.create_article(create_input("c1")).unwrap();
    assert_eq!(again.session.id, created.session.id);

    // Outra conversa ativa é recusada.
    let error = database.create_article(create_input("c2")).unwrap_err();
    assert_eq!(error.code, "ACTIVE_SESSION_EXISTS");

    // Artigo inválido é recusado antes de criar.
    let mut empty = create_input("c3");
    empty.text = "   ".into();
    assert_eq!(
        database.create_article(empty).unwrap_err().code,
        "INVALID_INPUT"
    );
}

#[test]
fn repeated_command_id_applies_once_and_stale_version_is_refused() {
    let (_directory, database) = database();
    let created = database.create_article(create_input("c1")).unwrap();
    let first = database
        .apply_article(&command(&created, "r1"), authorize())
        .unwrap();
    assert_eq!(first.session.state, ArticleState::AwaitingTutor);
    assert_eq!(first.session.version, 2);
    assert_eq!(
        first.session.authorization.as_ref().unwrap().destination,
        DESTINATION
    );

    // Mesmo ID de comando (mesmo com versão antiga): sem segunda aplicação.
    let replay = database
        .apply_article(&command(&created, "r1"), authorize())
        .unwrap();
    assert_eq!(replay.session.version, 2);
    assert_eq!(replay.session.state, ArticleState::AwaitingTutor);

    // ID novo com versão obsoleta.
    let stale = database
        .apply_article(&command(&created, "r2"), ArticleMutation::Revoke)
        .unwrap_err();
    assert_eq!(stale.code, "STALE_SOURCE");
    assert_eq!(
        database
            .article_snapshot(&created.session.id)
            .unwrap()
            .session
            .version,
        2
    );
}

#[test]
fn turns_versions_and_transcripts_are_persisted_in_order() {
    let (_directory, database) = database();
    let s = database.create_article(create_input("c1")).unwrap();
    let id = s.session.id.clone();
    let s = database
        .apply_article(&command(&s, "r1"), authorize())
        .unwrap();
    let s = database
        .apply_article(
            &command(&s, "r2"),
            ArticleMutation::AddTutorTurn {
                text: "What is the main argument?".into(),
            },
        )
        .unwrap();
    assert_eq!(s.session.state, ArticleState::Ready);
    let s = database
        .apply_article(
            &command(&s, "r3"),
            ArticleMutation::StartRecording {
                turn_id: "t-user-1".into(),
                audio_path: format!("articles/{id}/t-user-1.wav"),
            },
        )
        .unwrap();
    assert_eq!(s.session.state, ArticleState::Recording);
    let s = database
        .apply_article(&command(&s, "r4"), ArticleMutation::StopRecording)
        .unwrap();
    assert_eq!(s.session.state, ArticleState::Transcribing);
    let s = database
        .apply_article(
            &command(&s, "r5"),
            ArticleMutation::RecordTranscript {
                turn_id: "t-user-1".into(),
                text: Some("Remote work helps focus.".into()),
            },
        )
        .unwrap();
    assert_eq!(s.session.state, ArticleState::AwaitingTutor);
    assert_eq!(s.session.answers, 1);
    assert_eq!(s.turns.len(), 2);
    assert_eq!(s.turns[0].speaker, ArticleSpeaker::Tutor);
    assert_eq!(s.turns[1].speaker, ArticleSpeaker::User);
    assert_eq!(
        s.turns[1].version.source,
        Some(ArticleVersionSource::AutomaticSent)
    );
    assert!(s.turns[1].audio_available);
}

#[test]
fn empty_transcript_needs_review_and_manual_text_is_a_distinct_version() {
    let (_directory, database) = database();
    let s = database.create_article(create_input("c1")).unwrap();
    let id = s.session.id.clone();
    let mut s = database
        .apply_article(&command(&s, "r1"), authorize())
        .unwrap();
    for (n, mutation) in [
        ArticleMutation::AddTutorTurn { text: "Q?".into() },
        ArticleMutation::StartRecording {
            turn_id: "u1".into(),
            audio_path: format!("articles/{id}/u1.wav"),
        },
        ArticleMutation::StopRecording,
        ArticleMutation::RecordTranscript {
            turn_id: "u1".into(),
            text: Some("   ".into()),
        },
    ]
    .into_iter()
    .enumerate()
    {
        s = database
            .apply_article(&command(&s, &format!("x{n}")), mutation)
            .unwrap();
    }
    assert_eq!(s.session.state, ArticleState::NeedsReview);
    assert_eq!(s.session.answers, 0, "sem versão enviável não há resposta");
    let s = database
        .apply_article(
            &command(&s, "m1"),
            ArticleMutation::ManualText {
                turn_id: "u1".into(),
                text: "My manual text.".into(),
            },
        )
        .unwrap();
    assert_eq!(s.session.state, ArticleState::AwaitingTutor);
    let user = s
        .turns
        .iter()
        .find(|t| t.speaker == ArticleSpeaker::User)
        .unwrap();
    assert_eq!(
        user.version.source,
        Some(ArticleVersionSource::ManualReview)
    );
}

#[test]
fn sources_cannot_be_updated_or_deleted() {
    let (_directory, database) = database();
    let s = database.create_article(create_input("c1")).unwrap();
    let id = s.session.id.clone();
    let s = database
        .apply_article(&command(&s, "r1"), authorize())
        .unwrap();
    database
        .apply_article(
            &command(&s, "r2"),
            ArticleMutation::AddTutorTurn { text: "Q?".into() },
        )
        .unwrap();
    database
        .with_connection(|c| {
            for sql in [
                "UPDATE article_sessions SET article_text = 'x'",
                "UPDATE article_sessions SET title = 'x'",
                "UPDATE article_turns SET speaker = 'user'",
                "UPDATE article_versions SET text = 'x'",
                "DELETE FROM article_turns",
                "DELETE FROM article_versions",
            ] {
                assert!(c.execute(sql, []).is_err(), "{sql} devia falhar");
            }
            let n: i64 = c
                .query_row(
                    "SELECT count(*) FROM article_turns WHERE session_id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn artifacts_stay_inside_the_session_directory_and_delete_is_logical_first() {
    let (_directory, database) = database();
    let s = database.create_article(create_input("c1")).unwrap();
    let id = s.session.id.clone();
    let s = database
        .apply_article(&command(&s, "r1"), authorize())
        .unwrap();
    let s = database
        .apply_article(
            &command(&s, "r2"),
            ArticleMutation::AddTutorTurn { text: "Q?".into() },
        )
        .unwrap();
    for bad in [
        "../x.wav",
        "articles/other/x.wav",
        "sessions/x.wav",
        "/abs/x.wav",
    ] {
        let error = database
            .apply_article(
                &command(&s, "bad"),
                ArticleMutation::StartRecording {
                    turn_id: "u".into(),
                    audio_path: bad.into(),
                },
            )
            .unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT", "{bad}");
    }
    let s = database
        .apply_article(
            &command(&s, "r3"),
            ArticleMutation::StartRecording {
                turn_id: "u1".into(),
                audio_path: format!("articles/{id}/u1.wav"),
            },
        )
        .unwrap();
    let s = database
        .apply_article(&command(&s, "r4"), ArticleMutation::StopRecording)
        .unwrap();

    // Eliminação lógica: bloqueia novas operações e lista só ficheiros exclusivos.
    database
        .apply_article(&command(&s, "d1"), ArticleMutation::Delete)
        .unwrap();
    let paths = database.pending_article_artifacts(&id).unwrap();
    assert_eq!(paths, vec![format!("articles/{id}/u1.wav")]);
    let blocked = database
        .apply_article(&command(&s, "d2"), ArticleMutation::StopRecording)
        .unwrap_err();
    assert_eq!(blocked.code, "ARTICLE_DELETED");

    // Enquanto houver ficheiros por remover, as linhas ficam (limpeza repetível).
    assert!(!database.purge_article_if_clean(&id).unwrap());
    database.mark_article_artifact_removed(&paths[0]).unwrap();
    assert!(database.purge_article_if_clean(&id).unwrap());
    assert!(database.article_snapshot(&id).is_err());
    // Repetir a limpeza é inofensivo.
    assert!(database.purge_article_if_clean(&id).unwrap());
    // Já não existe sessão ativa: nova conversa permitida.
    assert!(database.create_article(create_input("c9")).is_ok());
}

/// Percorre a conversa por mutações encadeadas, com um requestId novo em cada passo.
struct Flow {
    database: Database,
    snapshot: ArticleSnapshot,
    n: u32,
}

impl Flow {
    fn new(text: &str) -> (tempfile::TempDir, Self) {
        let (directory, database) = database();
        let mut input = create_input("c1");
        input.text = text.into();
        let snapshot = database.create_article(input).unwrap();
        (
            directory,
            Flow {
                database,
                snapshot,
                n: 0,
            },
        )
    }

    fn id(&self) -> String {
        self.snapshot.session.id.clone()
    }

    fn apply(&mut self, mutation: ArticleMutation) -> &ArticleSnapshot {
        self.try_apply(mutation).unwrap()
    }

    fn try_apply(
        &mut self,
        mutation: ArticleMutation,
    ) -> Result<&ArticleSnapshot, language_assistant_tutor::domain::AppError> {
        self.n += 1;
        let request = format!("flow-{}", self.n);
        self.snapshot = self
            .database
            .apply_article(&command(&self.snapshot, &request), mutation)?;
        Ok(&self.snapshot)
    }

    fn authorize_and_ask(&mut self) {
        self.apply(authorize());
        self.apply(ArticleMutation::AddTutorTurn { text: "Q?".into() });
    }

    fn start(&mut self, turn: &str) {
        let path = format!("articles/{}/{turn}.wav", self.id());
        self.apply(ArticleMutation::StartRecording {
            turn_id: turn.into(),
            audio_path: path,
        });
    }

    fn answer(&mut self, turn: &str, text: &str) {
        self.start(turn);
        self.apply(ArticleMutation::StopRecording);
        self.apply(ArticleMutation::RecordTranscript {
            turn_id: turn.into(),
            text: Some(text.into()),
        });
    }
}

fn failure() -> ArticleError {
    ArticleError {
        code: "AI_UNAVAILABLE".into(),
        message: "falhou".into(),
        retryable: true,
    }
}

#[test]
fn retry_goes_back_to_the_step_that_failed() {
    // Feedback final falhado: repete o relatório.
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "My answer.");
    flow.apply(ArticleMutation::AddTutorTurn {
        text: "More?".into(),
    });
    flow.apply(ArticleMutation::Finish);
    let failed = flow.apply(ArticleMutation::Fail { error: failure() });
    assert_eq!(failed.session.state, ArticleState::RecoverableError);
    assert_eq!(
        failed.session.error.as_ref().unwrap().code,
        "AI_UNAVAILABLE"
    );
    // Pausar e retomar não perde a origem da falha.
    flow.apply(ArticleMutation::Pause);
    flow.apply(ArticleMutation::Resume);
    let retried = flow.apply(ArticleMutation::Retry);
    assert_eq!(retried.session.state, ArticleState::Finishing);
    assert!(retried.session.error.is_none());

    // Resposta do tutor falhada: repete a resposta.
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "My answer.");
    flow.apply(ArticleMutation::Fail { error: failure() });
    assert_eq!(
        flow.apply(ArticleMutation::Retry).session.state,
        ArticleState::AwaitingTutor
    );

    // Gravação interrompida: o turno não tem texto, volta a gravar.
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.start("u1");
    flow.apply(ArticleMutation::Fail { error: failure() });
    assert_eq!(
        flow.apply(ArticleMutation::Retry).session.state,
        ArticleState::Ready
    );
    // Sem falha registada não há o que repetir.
    assert!(flow.try_apply(ArticleMutation::Retry).is_err());
}

#[test]
fn text_waiting_for_review_never_reaches_the_tutor_or_the_limits() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    let id = flow.id();
    flow.answer("u1", &"a".repeat(8_001));
    // Transcrição excessiva fica à espera de revisão.
    assert_eq!(flow.snapshot.session.state, ArticleState::NeedsReview);
    assert_eq!(flow.snapshot.session.answers, 0);
    // O texto continua visível para o utilizador rever...
    let user = flow
        .snapshot
        .turns
        .iter()
        .find(|t| t.speaker == ArticleSpeaker::User)
        .unwrap();
    assert_eq!(user.id, "u1");
    assert_eq!(user.version.text.len(), 8_001);
    // ...mas terminar não o envia nem o conta.
    flow.apply(ArticleMutation::Finish);
    let context = flow.database.article_context(&id).unwrap();
    assert_eq!(context.turns.len(), 1, "só a pergunta do tutor");
    assert_eq!(context.turn_ids.len(), context.turns.len());
    assert_eq!(context.source_version_ids.len(), context.turns.len());
    assert!(context.turns.iter().all(|t| !t.text.contains("aaaa")));
}

#[test]
fn context_ties_each_turn_to_its_turn_and_version_ids() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "My answer.");
    let id = flow.id();
    let context = flow.database.article_context(&id).unwrap();
    assert_eq!(context.turns.len(), 2);
    assert_eq!(context.turn_ids[1], "u1");
    assert_eq!(
        context.source_version_ids[1],
        flow.snapshot.turns[1].version.id
    );
    assert_ne!(context.turn_ids[1], context.source_version_ids[1]);
}

#[test]
fn reauthorizing_resumes_the_right_step() {
    // Última fala do tutor: pronto para gravar.
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.apply(ArticleMutation::Revoke);
    assert_eq!(flow.apply(authorize()).session.state, ArticleState::Ready);

    // Última fala do utilizador (à espera do tutor): volta a pedir a resposta.
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "My answer.");
    flow.apply(ArticleMutation::Revoke);
    assert_eq!(
        flow.apply(authorize()).session.state,
        ArticleState::AwaitingTutor
    );

    // Resposta por rever: continua por rever.
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", &"a".repeat(8_001));
    flow.apply(ArticleMutation::Revoke);
    assert_eq!(
        flow.apply(authorize()).session.state,
        ArticleState::NeedsReview
    );
}

#[test]
fn only_the_current_turn_accepts_a_transcript_or_a_correction() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "First answer.");
    flow.apply(ArticleMutation::AddTutorTurn {
        text: "And?".into(),
    });
    flow.start("u2");
    flow.apply(ArticleMutation::StopRecording);
    // Transcrição atrasada de uma resposta antiga.
    let stale = flow
        .try_apply(ArticleMutation::RecordTranscript {
            turn_id: "u1".into(),
            text: Some("Rewritten.".into()),
        })
        .unwrap_err();
    assert_eq!(stale.code, "STALE_SOURCE");
    assert_eq!(flow.snapshot.session.state, ArticleState::Transcribing);
    assert_eq!(flow.snapshot.turns[1].version.text, "First answer.");
    // Resposta vazia leva a revisão; corrigir outro turno é recusado.
    flow.apply(ArticleMutation::RecordTranscript {
        turn_id: "u2".into(),
        text: None,
    });
    assert_eq!(flow.snapshot.session.state, ArticleState::NeedsReview);
    let stale = flow
        .try_apply(ArticleMutation::ManualText {
            turn_id: "u1".into(),
            text: "Rewritten.".into(),
        })
        .unwrap_err();
    assert_eq!(stale.code, "STALE_SOURCE");
    assert_eq!(flow.snapshot.turns[1].version.text, "First answer.");
    // O turno atual aceita a correção.
    flow.apply(ArticleMutation::ManualText {
        turn_id: "u2".into(),
        text: "Second answer.".into(),
    });
    assert_eq!(flow.snapshot.session.state, ArticleState::AwaitingTutor);
}

#[test]
fn a_full_context_of_multibyte_text_still_allows_the_final_feedback() {
    // Pior caso: artigo no máximo de bytes, pergunta de abertura, resposta e
    // réplica do tutor, todas em caracteres de 4 bytes.
    let (_d, mut flow) = Flow::new(&"𝄞".repeat(MAX_ARTICLE_BYTES / 4));
    flow.apply(authorize());
    flow.apply(ArticleMutation::AddTutorTurn {
        text: "𝄞".repeat(300),
    });
    let id = flow.id();
    assert!(!flow.snapshot.session.limit_reached);
    flow.answer("u1", &"𝄞".repeat(MAX_ANSWER_BYTES / 4));
    assert_eq!(flow.snapshot.session.state, ArticleState::AwaitingTutor);
    flow.apply(ArticleMutation::AddTutorTurn {
        text: "𝄞".repeat(MAX_TUTOR_REPLY_BYTES / 4),
    });
    flow.apply(ArticleMutation::Finish);
    let context = flow.database.article_context(&id).unwrap();
    build_article_body(
        &AiSettingsView::default(),
        ArticleAiTask::Feedback,
        &context,
    )
    .expect("o feedback nunca fica bloqueado pelo limite");

    // Resposta acima do limite em bytes é recusada para revisão (não enviada).
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", &"𝄞".repeat(MAX_ANSWER_BYTES / 4 + 1));
    assert_eq!(flow.snapshot.session.state, ArticleState::NeedsReview);
}

#[test]
fn a_reused_request_id_is_not_a_silent_success() {
    let (_d, mut flow) = Flow::new("Article.");
    let id = flow.id();
    flow.apply(authorize());
    let authorize_request = format!("flow-{}", flow.n);
    let version = flow.snapshot.session.version;
    let cmd = |request: &str, session: &str| ArticleCommand {
        request_id: request.into(),
        session_id: session.into(),
        expected_version: version,
    };
    // Mesmo requestId noutra operação: recusado, nada aplicado.
    let reused = flow
        .database
        .apply_article(&cmd(&authorize_request, &id), ArticleMutation::Revoke)
        .unwrap_err();
    assert_eq!(reused.code, "INVALID_INPUT");
    assert_eq!(
        flow.database.article_snapshot(&id).unwrap().session.state,
        ArticleState::AwaitingTutor
    );
    // O requestId da criação também não serve para outra operação.
    let reused = flow
        .database
        .apply_article(&cmd("c1", &id), ArticleMutation::Revoke)
        .unwrap_err();
    assert_eq!(reused.code, "INVALID_INPUT");
    // Nem para outra sessão.
    let other = flow
        .database
        .apply_article(&cmd(&authorize_request, "outra"), authorize())
        .unwrap_err();
    assert_eq!(other.code, "INVALID_INPUT");
    // Repetir exatamente o mesmo comando continua a ser idempotente.
    let same = flow
        .database
        .apply_article(&cmd(&authorize_request, &id), authorize())
        .unwrap();
    assert_eq!(same.session.version, version);

    // Depois de eliminada e limpa, repetir a criação não cria outra sessão.
    flow.apply(ArticleMutation::Delete);
    assert!(flow.database.purge_article_if_clean(&id).unwrap());
    let replay = flow
        .database
        .create_article(create_input("c1"))
        .unwrap_err();
    assert_eq!(replay.code, "ARTICLE_DELETED");
    let sessions: i64 = flow
        .database
        .with_connection(|c| {
            Ok(
                c.query_row("SELECT count(*) FROM article_sessions", [], |r| r.get(0))
                    .unwrap(),
            )
        })
        .unwrap();
    assert_eq!(sessions, 0);
}

#[test]
fn deleting_while_recording_is_refused_until_the_recording_stops() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    let id = flow.id();
    flow.start("u1");
    let refused = flow.try_apply(ArticleMutation::Delete).unwrap_err();
    assert_eq!(refused.code, "INVALID_INPUT");
    assert!(flow
        .database
        .pending_article_artifacts(&id)
        .unwrap()
        .is_empty());
    flow.apply(ArticleMutation::StopRecording);
    flow.apply(ArticleMutation::Delete);
    assert_eq!(
        flow.database.pending_article_artifacts(&id).unwrap(),
        vec![format!("articles/{id}/u1.wav")]
    );
}

#[test]
fn empty_user_turns_are_hidden_unless_they_await_manual_text() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.start("u1");
    // A gravar: só a pergunta do tutor aparece, sem balão vazio.
    assert_eq!(flow.snapshot.turns.len(), 1);
    flow.apply(ArticleMutation::StopRecording);
    assert_eq!(flow.snapshot.turns.len(), 1);
    // Transcrição vazia: o turno é necessário para o texto manual.
    flow.apply(ArticleMutation::RecordTranscript {
        turn_id: "u1".into(),
        text: None,
    });
    assert_eq!(flow.snapshot.turns.len(), 2);
    assert_eq!(flow.snapshot.turns[1].id, "u1");
    assert!(flow.snapshot.turns[1].version.text.is_empty());
}

#[test]
fn a_forked_branch_projects_the_ancestor_prefix_and_latest_versions() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "First answer.");
    flow.apply(ArticleMutation::AddTutorTurn {
        text: "Why?".into(),
    });
    flow.answer("u2", "Second answer.");
    let id = flow.id();
    flow.database
        .with_connection(|c| {
            c.execute_batch(&format!(
                "INSERT INTO article_branches (id, session_id, parent_branch_id, fork_turn_id, created_at)
                   SELECT 'b2', '{id}', active_branch_id, 'u1', 'now' FROM article_sessions;
                 INSERT INTO article_turns (id, session_id, branch_id, ordinal, speaker, created_at)
                   VALUES ('b2-t', '{id}', 'b2', 4, 'tutor', 'now');
                 INSERT INTO article_versions (id, session_id, turn_id, branch_id, seq, source, text, created_at)
                   VALUES ('b2-v1', '{id}', 'u1', 'b2', 1, 'manual_review', 'Corrected first.', 'now'),
                          ('b2-v2', '{id}', 'b2-t', 'b2', 1, NULL, 'Branch question?', 'now');
                 UPDATE article_sessions SET active_branch_id = 'b2';"
            ))
            .unwrap();
            Ok(())
        })
        .unwrap();
    let snapshot = flow.database.article_snapshot(&id).unwrap();
    let turns: Vec<_> = snapshot
        .turns
        .iter()
        .map(|t| (t.version.text.as_str(), t.from_previous_branch))
        .collect();
    assert_eq!(
        turns,
        vec![
            ("Q?", true),
            ("Corrected first.", true),
            ("Branch question?", false)
        ]
    );
    let context = flow.database.article_context(&id).unwrap();
    assert_eq!(context.branch_id, "b2");
    assert_eq!(context.turn_ids[1], "u1");
    assert_eq!(context.source_version_ids[1], "b2-v1");
}

#[test]
fn reauthorizing_after_a_failed_final_feedback_resumes_the_report() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "My answer.");
    flow.apply(ArticleMutation::AddTutorTurn {
        text: "More?".into(),
    });
    flow.apply(ArticleMutation::Finish);
    flow.apply(ArticleMutation::Fail { error: failure() });
    let revoked = flow.apply(ArticleMutation::Revoke);
    assert_eq!(revoked.session.state, ArticleState::Draft);
    assert!(revoked.session.authorization.is_none());
    // Sem nova autorização nada se repete; ao autorizar, o relatório retoma.
    assert_eq!(
        flow.apply(authorize()).session.state,
        ArticleState::Finishing
    );
    // A intenção de terminar não sobrevive a uma falha de outro tipo.
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.answer("u1", "My answer.");
    flow.apply(ArticleMutation::Fail { error: failure() });
    flow.apply(ArticleMutation::Revoke);
    assert_eq!(
        flow.apply(authorize()).session.state,
        ArticleState::AwaitingTutor
    );
}

#[test]
fn turn_indexes_stay_contiguous_after_an_abandoned_recording() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    flow.start("u1");
    flow.apply(ArticleMutation::Fail { error: failure() });
    flow.apply(ArticleMutation::Retry);
    flow.answer("u2", "Real answer.");
    let indexes: Vec<u32> = flow.snapshot.turns.iter().map(|t| t.index).collect();
    assert_eq!(indexes, vec![1, 2]);
    assert_eq!(flow.snapshot.turns[1].id, "u2");
}

#[test]
fn purging_keeps_only_the_create_receipt_and_prunes_old_ones() {
    let (_d, mut flow) = Flow::new("Article.");
    flow.authorize_and_ask();
    let id = flow.id();
    // Recibo antigo de uma conversa já limpa e outro recente.
    flow.database
        .with_connection(|c| {
            c.execute_batch(
                "INSERT INTO article_receipts VALUES ('old', 'gone-1', 'create', 1, '2020-01-01T00:00:00.000Z');
                 INSERT INTO article_receipts VALUES ('recent', 'gone-2', 'create', 1, strftime('%Y-%m-%dT%H:%M:%fZ','now'));",
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    flow.apply(ArticleMutation::Delete);
    assert!(flow.database.purge_article_if_clean(&id).unwrap());
    let ids: Vec<String> = flow
        .database
        .with_connection(|c| {
            let mut st = c
                .prepare("SELECT request_id FROM article_receipts ORDER BY request_id")
                .unwrap();
            let rows = st
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            Ok(rows)
        })
        .unwrap();
    // `c1` (criação desta sessão) e `recent` ficam; passos intermédios e `old` saem.
    assert_eq!(ids, vec!["c1".to_string(), "recent".to_string()]);
}
