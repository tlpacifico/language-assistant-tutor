//! Persistência das conversas sobre artigos: criação, mutações, recibos e fontes.

use language_assistant_tutor::article::{
    ArticleCommand, ArticleCreate, ArticleMutation, ArticleSnapshot, ArticleSpeaker, ArticleState,
    ArticleVersionSource, ARTICLE_NOTICE_VERSION,
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
