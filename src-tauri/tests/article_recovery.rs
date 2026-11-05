//! Correções em ramos e recuperação local depois de um reinício.

use language_assistant_tutor::article::{
    ArticleCommand, ArticleCreate, ArticleDimension, ArticleMutation, ArticleOutcome,
    ArticleReport, ArticleSnapshot, ArticleSpeaker, ArticleState, DimensionStatus,
    ARTICLE_NOTICE_VERSION,
};
use language_assistant_tutor::db::Database;

const DESTINATION: &str = "https://opencode.ai/inference/go/openai/v1";

struct Flow {
    _directory: tempfile::TempDir,
    database: Database,
    snapshot: ArticleSnapshot,
    n: u32,
}

impl Flow {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("diretório temporário");
        let database = Database::open(directory.path().join("tutor.sqlite3")).expect("abrir");
        let snapshot = database
            .create_article(ArticleCreate {
                request_id: "create".into(),
                title: Some("Remote work".into()),
                text: "Remote work changed collaboration.".into(),
            })
            .unwrap();
        Flow {
            _directory: directory,
            database,
            snapshot,
            n: 0,
        }
    }

    fn id(&self) -> String {
        self.snapshot.session.id.clone()
    }

    fn command(&mut self) -> ArticleCommand {
        self.n += 1;
        ArticleCommand {
            request_id: format!("r{}", self.n),
            session_id: self.id(),
            expected_version: self.snapshot.session.version,
        }
    }

    fn apply(&mut self, mutation: ArticleMutation) -> &ArticleSnapshot {
        let command = self.command();
        self.snapshot = self.database.apply_article(&command, mutation).unwrap();
        &self.snapshot
    }

    fn start(&mut self) {
        self.apply(ArticleMutation::Authorize {
            destination: DESTINATION.into(),
            notice_version: ARTICLE_NOTICE_VERSION.into(),
        });
        self.apply(ArticleMutation::AddTutorTurn { text: "Q1?".into() });
    }

    fn answer(&mut self, turn: &str, text: &str) {
        let path = format!("articles/{}/{turn}/original.wav", self.id());
        self.apply(ArticleMutation::StartRecording {
            turn_id: turn.into(),
            audio_path: path,
        });
        self.apply(ArticleMutation::StopRecording);
        self.apply(ArticleMutation::RecordTranscript {
            turn_id: turn.into(),
            text: Some(text.into()),
        });
    }

    fn tutor(&mut self, text: &str) {
        self.apply(ArticleMutation::AddTutorTurn { text: text.into() });
    }

    /// Três respostas, relatório e conversa concluída.
    fn completed(&mut self) {
        self.start();
        self.answer("u1", "First answer about focus.");
        self.tutor("Q2?");
        self.answer("u2", "Second answer about teams.");
        self.tutor("Q3?");
        self.answer("u3", "Third answer about trust.");
        self.apply(ArticleMutation::Finish);
        self.report("rep-1");
    }

    /// Fecha o pedido de feedback pendente com um relatório mínimo.
    fn report(&mut self, id: &str) {
        let config = "{}";
        let ticket = self
            .database
            .begin_article_request(&self.id(), config)
            .unwrap()
            .expect("pedido de feedback");
        let dimension = ArticleDimension {
            status: DimensionStatus::InsufficientEvidence,
            summary: "Pouco texto.".into(),
            evidence: vec![],
        };
        let report = ArticleReport {
            id: id.into(),
            branch_id: ticket.context.branch_id.clone(),
            version_ids: ticket.context.source_version_ids.clone(),
            coherence: dimension.clone(),
            cohesion: dimension.clone(),
            development: dimension.clone(),
            interaction: dimension,
            improvements: vec![],
        };
        self.snapshot = self
            .database
            .finish_article_request(
                &ticket.request_id,
                ArticleOutcome::Report(Box::new(report)),
                None,
            )
            .unwrap()
            .unwrap();
    }
}

fn texts(snapshot: &ArticleSnapshot) -> Vec<String> {
    snapshot
        .turns
        .iter()
        .map(|t| t.version.text.clone())
        .collect()
}

#[test]
fn correcting_the_first_answer_forks_a_branch_and_keeps_the_old_one() {
    let mut flow = Flow::new();
    flow.completed();
    let old_branch = flow.snapshot.session.active_branch_id.clone();
    assert_eq!(flow.snapshot.session.state, ArticleState::Completed);
    let old_texts = texts(&flow.snapshot);
    assert_eq!(flow.snapshot.report.as_ref().unwrap().id, "rep-1");

    let corrected = flow
        .apply(ArticleMutation::Correct {
            turn_id: "u1".into(),
            text: "Corrected first answer about focus.".into(),
        })
        .clone();
    // Novo ramo, sem as respostas posteriores; a regeneração fica pendente de ação.
    assert_ne!(corrected.session.active_branch_id, old_branch);
    assert_eq!(corrected.session.state, ArticleState::RecoverableError);
    assert_eq!(
        texts(&corrected),
        vec!["Q1?", "Corrected first answer about focus."]
    );
    assert!(corrected.turns.iter().all(|t| t.from_previous_branch));
    assert!(
        corrected.report.is_none(),
        "o relatório novo ainda não existe"
    );
    let context = flow.database.article_context(&flow.id()).unwrap();
    assert_eq!(context.turns.len(), 2);

    // O ramo antigo continua intacto e reconstruível.
    let old_context = flow
        .database
        .project_article_branch(&flow.id(), &old_branch)
        .unwrap();
    assert_eq!(old_context.turns.len(), 6);
    assert_eq!(old_context.turns[1].text, "First answer about focus.");
    let old_reports: i64 = flow
        .database
        .with_connection(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM article_reports WHERE branch_id = ?1 AND id = 'rep-1'",
                [&old_branch],
                |r| r.get(0),
            )
            .unwrap())
        })
        .unwrap();
    assert_eq!(old_reports, 1);

    // Repetir regenera o relatório no ramo novo.
    flow.apply(ArticleMutation::Retry);
    assert_eq!(flow.snapshot.session.state, ArticleState::Finishing);
    flow.report("rep-2");
    assert_eq!(flow.snapshot.session.state, ArticleState::Completed);
    assert_eq!(flow.snapshot.report.as_ref().unwrap().id, "rep-2");
    assert_eq!(old_texts.len(), 6);
}

#[test]
fn successive_corrections_build_a_chain_of_branches() {
    let mut flow = Flow::new();
    flow.completed();
    flow.apply(ArticleMutation::Correct {
        turn_id: "u2".into(),
        text: "Corrected second answer.".into(),
    });
    flow.apply(ArticleMutation::Retry);
    flow.report("rep-2");
    let first_fork = flow.snapshot.clone();
    assert_eq!(
        texts(&first_fork),
        vec![
            "Q1?",
            "First answer about focus.",
            "Q2?",
            "Corrected second answer."
        ]
    );
    // Segunda correção, agora de um turno herdado do ramo-raiz.
    flow.apply(ArticleMutation::Correct {
        turn_id: "u1".into(),
        text: "Twice corrected first.".into(),
    });
    assert_eq!(texts(&flow.snapshot), vec!["Q1?", "Twice corrected first."]);
    let branches: i64 = flow
        .database
        .with_connection(|c| {
            Ok(
                c.query_row("SELECT count(*) FROM article_branches", [], |r| r.get(0))
                    .unwrap(),
            )
        })
        .unwrap();
    assert_eq!(branches, 3);
    // O ramo da primeira correção continua a projetar o mesmo percurso.
    let context = flow
        .database
        .project_article_branch(&flow.id(), &first_fork.session.active_branch_id)
        .unwrap();
    assert_eq!(context.turns.len(), 4);
    assert_eq!(context.turns[3].text, "Corrected second answer.");
}

#[test]
fn only_a_completed_conversation_accepts_a_correction_of_a_user_answer() {
    let mut flow = Flow::new();
    flow.start();
    flow.answer("u1", "Answer.");
    let command = flow.command();
    // Ainda não terminou: correções só no final.
    let early = flow
        .database
        .apply_article(
            &command,
            ArticleMutation::Correct {
                turn_id: "u1".into(),
                text: "Changed.".into(),
            },
        )
        .unwrap_err();
    assert_eq!(early.code, "INVALID_INPUT");

    flow.tutor("More?");
    flow.apply(ArticleMutation::Finish);
    flow.report("rep");
    for (turn, text) in [("nope", "Changed."), ("u1", "   "), ("u1", "Answer.")] {
        let command = flow.command();
        let error = flow
            .database
            .apply_article(
                &command,
                ArticleMutation::Correct {
                    turn_id: turn.into(),
                    text: text.into(),
                },
            )
            .unwrap_err();
        assert!(
            error.code == "INVALID_INPUT" || error.code == "STALE_SOURCE",
            "{turn}/{text}: {}",
            error.code
        );
    }
    // O tutor não é corrigível.
    let tutor_turn = flow.snapshot.turns[0].id.clone();
    assert_eq!(flow.snapshot.turns[0].speaker, ArticleSpeaker::Tutor);
    let command = flow.command();
    assert!(flow
        .database
        .apply_article(
            &command,
            ArticleMutation::Correct {
                turn_id: tutor_turn,
                text: "Changed.".into(),
            },
        )
        .is_err());
    assert_eq!(flow.snapshot.session.state, ArticleState::Completed);
}

#[test]
fn a_correction_is_refused_while_another_conversation_is_active() {
    let mut flow = Flow::new();
    flow.completed();
    flow.database
        .create_article(ArticleCreate {
            request_id: "another".into(),
            title: None,
            text: "Another article.".into(),
        })
        .unwrap();
    let command = flow.command();
    let error = flow
        .database
        .apply_article(
            &command,
            ArticleMutation::Correct {
                turn_id: "u1".into(),
                text: "Changed.".into(),
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "ACTIVE_SESSION_EXISTS");
    assert_eq!(
        flow.database
            .article_snapshot(&flow.id())
            .unwrap()
            .session
            .state,
        ArticleState::Completed
    );
}

// -- recuperação depois de um reinício ---------------------------------------

fn failure_code(snapshot: &ArticleSnapshot) -> String {
    snapshot.session.error.as_ref().unwrap().code.clone()
}

#[test]
fn a_pending_request_becomes_an_explicit_recoverable_interruption() {
    let mut flow = Flow::new();
    flow.start();
    flow.answer("u1", "My answer.");
    let ticket = flow
        .database
        .begin_article_request(&flow.id(), "{}")
        .unwrap()
        .expect("pedido de resposta");
    let _ = ticket;
    // Fecho abrupto: o pedido ficou pendente na base.
    flow.database.recover_articles().unwrap();
    let after = flow.database.article_snapshot(&flow.id()).unwrap();
    assert_eq!(after.session.state, ArticleState::RecoverableError);
    assert_eq!(failure_code(&after), "AI_INTERRUPTED");
    // O consumo não é dado como zero.
    let usage = after
        .session
        .usage
        .clone()
        .expect("há um pedido interrompido");
    assert_eq!(usage.input_tokens, None);
    let pending: i64 = flow
        .database
        .with_connection(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM article_requests WHERE state = 'pending'",
                [],
                |r| r.get(0),
            )
            .unwrap())
        })
        .unwrap();
    assert_eq!(pending, 0);
    // Repetir é uma ação explícita e cria nova tentativa identificada.
    flow.snapshot = after;
    flow.apply(ArticleMutation::Retry);
    let second = flow
        .database
        .begin_article_request(&flow.id(), "{}")
        .unwrap()
        .unwrap();
    assert_eq!(second.attempt, 2);
}

#[test]
fn an_interrupted_recording_is_flagged_and_can_be_recorded_again() {
    let mut flow = Flow::new();
    flow.start();
    let path = format!("articles/{}/u1/original.wav", flow.id());
    flow.apply(ArticleMutation::StartRecording {
        turn_id: "u1".into(),
        audio_path: path,
    });
    flow.database.recover_articles().unwrap();
    let after = flow.database.article_snapshot(&flow.id()).unwrap();
    assert_eq!(after.session.state, ArticleState::RecoverableError);
    assert_eq!(failure_code(&after), "CAPTURE_INTERRUPTED");
    assert_eq!(after.turns.len(), 1, "não aparece como resposta completa");
    flow.snapshot = after;
    flow.apply(ArticleMutation::Retry);
    assert_eq!(flow.snapshot.session.state, ArticleState::Ready);
}

#[test]
fn an_unfinished_transcription_asks_for_review_after_a_restart() {
    let mut flow = Flow::new();
    flow.start();
    let path = format!("articles/{}/u1/original.wav", flow.id());
    flow.apply(ArticleMutation::StartRecording {
        turn_id: "u1".into(),
        audio_path: path,
    });
    flow.apply(ArticleMutation::StopRecording);
    flow.database.recover_articles().unwrap();
    let after = flow.database.article_snapshot(&flow.id()).unwrap();
    assert_eq!(after.session.state, ArticleState::NeedsReview);
    // O áudio guardado continua associado ao turno.
    assert!(after
        .turns
        .iter()
        .any(|t| t.id == "u1" && t.audio_available));
}

#[test]
fn recovery_is_idempotent_and_never_touches_other_states() {
    let mut flow = Flow::new();
    flow.start();
    flow.database.recover_articles().unwrap();
    flow.database.recover_articles().unwrap();
    let after = flow.database.article_snapshot(&flow.id()).unwrap();
    assert_eq!(after.session.state, ArticleState::Ready);
    assert_eq!(after.session.version, flow.snapshot.session.version);
}

#[test]
fn deleting_can_be_repeated_and_resumes_the_pending_cleanup() {
    let mut flow = Flow::new();
    flow.start();
    let path = format!("articles/{}/u1/original.wav", flow.id());
    flow.apply(ArticleMutation::StartRecording {
        turn_id: "u1".into(),
        audio_path: path.clone(),
    });
    flow.apply(ArticleMutation::StopRecording);
    flow.apply(ArticleMutation::Delete);
    // Reinício a meio da limpeza: a sessão eliminada é devolvida para limpar.
    let pending = flow.database.recover_articles().unwrap();
    assert_eq!(pending, vec![flow.id()]);
    assert_eq!(
        flow.database.pending_article_artifacts(&flow.id()).unwrap(),
        vec![path.clone()]
    );
    // Repetir é inofensivo e só liberta depois de os ficheiros saírem.
    assert_eq!(flow.database.recover_articles().unwrap(), vec![flow.id()]);
    flow.database.mark_article_artifact_removed(&path).unwrap();
    assert!(flow.database.purge_article_if_clean(&flow.id()).unwrap());
    assert!(flow.database.recover_articles().unwrap().is_empty());
}
