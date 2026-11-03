use language_assistant_tutor::db::{Database, DATABASE_VERSION, MIGRATION_V1};
#[test]
fn populated_v1_migrates_without_loss() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sqlite");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(MIGRATION_V1).unwrap();
    c.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA user_version=1;
 INSERT INTO sessions VALUES ('s',NULL,1,1,'completed','local_v2',3,'now','done');
 INSERT INTO sessions VALUES ('s2',NULL,1,1,'in_progress','local_v2',1,'now',NULL);
 INSERT INTO questions VALUES ('q','Question','failure_learning','[]','[]',1,'now','now');
 INSERT INTO session_questions VALUES ('a','s','q',1,1,'Question','failure_learning','[]','[]','r',1);
 INSERT INTO session_questions VALUES ('a2','s2','q',1,1,'Question','failure_learning','[]','[]',NULL,0);
 INSERT INTO recordings VALUES ('r','a',1,'ready','s/a/r/original.wav',NULL,NULL,'now','now');
 INSERT INTO transcripts VALUES ('t','r','completed','Automatic text','[]','small','en','{}',1,2,'now');
 INSERT INTO reviews VALUES ('r',1,'t','automatic','Confirmed text','now');
 INSERT INTO confirmations VALUES ('c','a','r',1,'t','automatic','Confirmed text','now');
 INSERT INTO operations VALUES ('op','transcribe','running','s2','a2',NULL,1,'{}',NULL,'now',NULL);
 INSERT INTO command_receipts VALUES ('receipt','confirm_answer','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','s','c',3);").unwrap();
    std::fs::write(dir.path().join("original.wav"), b"unchanged audio").unwrap();
    drop(c);
    let db = Database::open(path).unwrap();
    assert_eq!(DATABASE_VERSION, 2);
    db.with_connection(|c| {
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM answer_slots WHERE id='a' AND role='initial'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        assert!(!c
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap());
        Ok(())
    })
    .unwrap();
    let old = db.load_session("s").unwrap();
    assert_eq!(
        old.questions[0].confirmation.as_ref().unwrap().text,
        "Confirmed text"
    );
    assert_eq!(
        old.questions[0].recordings[0].transcripts[0].text,
        "Automatic text"
    );
    assert_eq!(
        old.questions[0].recordings[0].original_file.as_deref(),
        Some("s/a/r/original.wav")
    );
    assert_eq!(db.active_session().unwrap().unwrap().summary.id, "s2");
    assert_eq!(
        std::fs::read(dir.path().join("original.wav")).unwrap(),
        b"unchanged audio"
    );
    db.with_connection(|c| {
        assert!(c
            .execute(
                "INSERT INTO answer_slots VALUES ('bad','a','s2','follow_up','Follow?',NULL,0)",
                []
            )
            .is_err());
        assert!(c
            .execute("UPDATE confirmations SET text='changed' WHERE id='c'", [])
            .is_err());
        c.execute("INSERT INTO answer_slots VALUES ('follow','a','s','follow_up','What did you learn?',NULL,0)",[]).unwrap();
        assert!(c.execute("INSERT INTO answer_slots VALUES ('follow2','a','s','follow_up','Another?',NULL,0)",[]).is_err());
        assert!(c.execute("UPDATE answer_slots SET selected_recording_id='r',selection_version=1 WHERE id='follow'",[]).is_err());
        let count: i64 = c
            .query_row(
                "SELECT count(*) FROM command_receipts WHERE request_id='receipt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn failed_v1_migration_rolls_back_without_recreating_database() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("bad.sqlite");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(MIGRATION_V1).unwrap();
    c.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA user_version=1; INSERT INTO sessions VALUES('s',NULL,1,1,'in_progress','local_v2',1,'now',NULL); INSERT INTO questions VALUES('q','Question','failure_learning','[]','[]',1,'now','now'); INSERT INTO session_questions VALUES('a','s','q',1,1,'Question','failure_learning','[]','[]','missing',1);").unwrap();
    drop(c);
    assert!(Database::open(path.clone()).is_err());
    let c = rusqlite::Connection::open(path).unwrap();
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let text: String = c
        .query_row("SELECT text FROM session_questions WHERE id='a'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(text, "Question");
    assert!(!c
        .prepare("SELECT name FROM sqlite_master WHERE name='answer_slots'")
        .unwrap()
        .exists([])
        .unwrap());
}
