//! Voz incremental do tutor: protocolo de blocos, validação da sequência,
//! reprodução em fila com cancelamento e cache local de voz.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::json;

use language_assistant_tutor::article::{
    begin_entry, clear_entry, finish_entry, read_cache, remove_incomplete, voice_key,
    ChunkRejection, ChunkSequence, ENGINE_ID,
};
use language_assistant_tutor::worker::protocol::{
    decode_response, encode_request, parse_result, SynthesizeStreamParams, WorkerOp,
    WorkerResponse, WorkerResult,
};
use language_assistant_tutor::worker::WorkerRequest;

fn directory() -> PathBuf {
    PathBuf::from(r"C:\data\articles\s1\voice-1")
}

fn chunk_line(index: u32, path: &str) -> String {
    json!({
        "protocolVersion": 1,
        "id": "voice-1",
        "event": "audio_chunk",
        "index": index,
        "audioPath": path,
        "sampleRate": 24000,
        "durationMs": 1200
    })
    .to_string()
}

#[test]
fn stream_and_cancel_requests_use_the_new_operations_only() {
    let request = WorkerRequest::synthesize_stream(
        "voice-1",
        SynthesizeStreamParams::new("Why does that matter?", directory()),
    )
    .unwrap();
    assert_eq!(request.op, WorkerOp::SynthesizeStream);
    let line = encode_request(&request.id, request.op, &request.params).unwrap();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["op"], "synthesize_stream");
    assert_eq!(value["params"]["voice"], "af_heart");
    assert_eq!(value["params"]["outputDir"], r"C:\data\articles\s1\voice-1");
    // O pedido antigo mantém o nome e o contrato.
    assert_eq!(WorkerOp::Synthesize.as_str(), "synthesize");
    let cancel = encode_request("cancel-1", WorkerOp::Cancel, &json!({"target": "voice-1"}));
    assert!(cancel.unwrap().contains("\"op\":\"cancel\""));
}

#[test]
fn audio_chunk_events_and_the_terminal_result_are_typed() {
    let line = chunk_line(0, r"C:\data\articles\s1\voice-1\chunk-0000.wav");
    match decode_response(&line).unwrap() {
        WorkerResponse::AudioChunk {
            id,
            index,
            audio_path,
            sample_rate,
            duration_ms,
        } => {
            assert_eq!(id, "voice-1");
            assert_eq!(index, 0);
            assert_eq!(audio_path, directory().join("chunk-0000.wav"));
            assert_eq!(sample_rate, 24000);
            assert_eq!(duration_ms, 1200);
        }
        other => panic!("esperava um bloco, veio {other:?}"),
    }
    // Um bloco sem caminho não respeita o protocolo.
    let broken = json!({
        "protocolVersion": 1, "id": "voice-1", "event": "audio_chunk", "index": 0
    })
    .to_string();
    assert_eq!(
        decode_response(&broken).unwrap_err().code,
        "WORKER_PROTOCOL"
    );

    let result = parse_result(
        WorkerOp::SynthesizeStream,
        json!({"chunks": 3, "sampleRate": 24000, "cancelled": false, "loadMs": 5, "processingMs": 90}),
    )
    .unwrap();
    match result {
        WorkerResult::SynthesisStream(done) => {
            assert_eq!(done.chunks, 3);
            assert!(!done.cancelled);
        }
        other => panic!("resultado inesperado: {other:?}"),
    }
}

#[test]
fn chunks_are_accepted_only_in_order_inside_their_own_directory() {
    let mut sequence = ChunkSequence::new(directory());
    let first = directory().join("chunk-0000.wav");
    assert_eq!(sequence.accept(0, &first).unwrap(), first);
    // Duplicado e fora de ordem.
    assert_eq!(
        sequence.accept(0, &first).unwrap_err(),
        ChunkRejection::Duplicate
    );
    assert_eq!(
        sequence
            .accept(2, &directory().join("chunk-0002.wav"))
            .unwrap_err(),
        ChunkRejection::OutOfOrder {
            expected: 1,
            got: 2
        }
    );
    // O seguinte correto continua a ser aceite depois das recusas.
    let second = directory().join("chunk-0001.wav");
    assert_eq!(sequence.accept(1, &second).unwrap(), second);
    assert_eq!(sequence.accepted(), 2);
}

#[test]
fn chunks_outside_the_operation_directory_or_with_the_wrong_name_are_refused() {
    let mut sequence = ChunkSequence::new(directory());
    for bad in [
        r"C:\data\articles\other\voice-1\chunk-0000.wav",
        r"C:\data\articles\s1\voice-1\..\voice-2\chunk-0000.wav",
        r"C:\data\articles\s1\voice-1\sub\chunk-0000.wav",
        r"C:\data\articles\s1\voice-1\chunk-0000.wav.part",
        r"C:\data\articles\s1\voice-1\other.wav",
    ] {
        let rejected = sequence.accept(0, Path::new(bad));
        assert!(
            matches!(
                rejected,
                Err(ChunkRejection::OutsideDirectory | ChunkRejection::BadName)
            ),
            "{bad}: {rejected:?}"
        );
    }
    // Nenhuma recusa avança a sequência.
    assert_eq!(sequence.accepted(), 0);
}

#[test]
fn shared_stream_examples_match_the_rust_protocol() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("contracts")
        .join("worker-v1.stream.examples.json");
    let examples: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let request = &examples["requests"]["synthesizeStream"];
    let params: SynthesizeStreamParams = serde_json::from_value(request["params"].clone()).unwrap();
    assert_eq!(params.voice, "af_heart");
    let line = examples["responses"]["audioChunk"].to_string();
    assert!(matches!(
        decode_response(&line).unwrap(),
        WorkerResponse::AudioChunk { index: 0, .. }
    ));
    let result = decode_response(&examples["responses"]["streamResult"].to_string()).unwrap();
    let WorkerResponse::Result {
        result: Some(value),
        ..
    } = result
    else {
        panic!("esperava um resultado");
    };
    assert!(matches!(
        parse_result(WorkerOp::SynthesizeStream, value).unwrap(),
        WorkerResult::SynthesisStream(_)
    ));
}

// -- cache local de voz ------------------------------------------------------

/// Escreve um bloco WAV PCM16 de 24 kHz com `frames` amostras.
fn write_chunk(dir: &Path, index: u32, frames: u32) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 24_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let path = dir.join(format!("chunk-{index:04}.wav"));
    let mut writer = WavWriter::create(&path, spec).expect("criar bloco WAV");
    for _ in 0..frames {
        writer.write_sample(0i16).expect("escrever amostra");
    }
    writer.finalize().expect("fechar bloco WAV");
    path
}

/// 2 400 amostras a 24 kHz: 100 ms exatos.
const FRAMES_100MS: u32 = 2_400;

#[test]
fn cache_key_changes_with_text_voice_speed_and_engine() {
    let base = voice_key("text", "af_heart", 10, ENGINE_ID);
    assert_eq!(base.len(), 64);
    assert!(base
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    assert_ne!(base, voice_key("other", "af_heart", 10, ENGINE_ID), "texto");
    assert_ne!(base, voice_key("text", "af_bella", 10, ENGINE_ID), "voz");
    assert_ne!(
        base,
        voice_key("text", "af_heart", 12, ENGINE_ID),
        "velocidade"
    );
    assert_ne!(
        base,
        voice_key("text", "af_heart", 10, "outro-motor"),
        "motor"
    );
}

#[test]
fn an_entry_without_a_complete_manifest_is_not_reused() {
    let session = tempfile::tempdir().unwrap();
    let session_dir = session.path();
    let key = voice_key("text", "af_heart", 10, ENGINE_ID);
    let entry = begin_entry(session_dir, &key).unwrap();
    write_chunk(&entry, 0, FRAMES_100MS);
    // Sem manifesto: não é reutilizada.
    assert!(read_cache(session_dir, &key).is_none());

    // Manifesto incompleto: também não é reutilizada.
    let manifest = json!({
        "version": 1,
        "key": key,
        "voice": "af_heart",
        "speedX10": 10,
        "engine": ENGINE_ID,
        "textSha256": "x".repeat(64),
        "complete": false,
        "chunks": [{ "index": 0, "durationMs": 100 }]
    });
    std::fs::write(entry.join("manifest.json"), manifest.to_string()).unwrap();
    assert!(read_cache(session_dir, &key).is_none());
}

#[test]
fn corrupt_or_missing_chunk_invalidates_the_entry() {
    let session = tempfile::tempdir().unwrap();
    let session_dir = session.path();
    let key = voice_key("text", "af_heart", 10, ENGINE_ID);
    let entry = begin_entry(session_dir, &key).unwrap();
    write_chunk(&entry, 0, FRAMES_100MS);
    finish_entry(&entry, "text", "af_heart", 10, ENGINE_ID, &[(0, 100)]).unwrap();

    let cached = read_cache(session_dir, &key).expect("entrada completa");
    assert_eq!(cached.chunks.len(), 1);
    assert_eq!(cached.chunks[0].index, 0);
    assert_eq!(cached.chunks[0].duration_ms, 100);
    assert_eq!(cached.chunks[0].path, entry.join("chunk-0000.wav"));
    assert_eq!(cached.total_duration_ms(), 100);

    // Bloco corrompido: a entrada deixa de ser reutilizável.
    std::fs::write(entry.join("chunk-0000.wav"), b"nao e um wav").unwrap();
    assert!(read_cache(session_dir, &key).is_none());

    // Bloco em falta também invalida a leitura.
    std::fs::remove_file(entry.join("chunk-0000.wav")).unwrap();
    assert!(read_cache(session_dir, &key).is_none());

    // Concluir com um bloco em falta devolve erro sem deixar manifesto completo.
    let entry = begin_entry(session_dir, &key).unwrap();
    write_chunk(&entry, 0, FRAMES_100MS);
    std::fs::remove_file(entry.join("chunk-0000.wav")).unwrap();
    assert!(finish_entry(&entry, "text", "af_heart", 10, ENGINE_ID, &[(0, 100)]).is_err());
    assert!(read_cache(session_dir, &key).is_none());
}

#[test]
fn a_key_outside_the_cache_directory_is_refused() {
    let session = tempfile::tempdir().unwrap();
    let session_dir = session.path();
    for bad in [
        "curto".to_string(),
        "../escape".to_string(),
        "Z".repeat(64),
        format!("{}g", "a".repeat(63)),
    ] {
        let error = begin_entry(session_dir, &bad).unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT", "{bad}");
        assert!(read_cache(session_dir, &bad).is_none(), "{bad}");
    }
    // A pasta `voice-cache/` nunca foi criada por uma chave recusada.
    assert!(!session_dir.join("voice-cache").exists());
}

#[test]
fn a_partial_entry_is_removed_before_regeneration() {
    let session = tempfile::tempdir().unwrap();
    let session_dir = session.path();
    let key = voice_key("text", "af_heart", 10, ENGINE_ID);

    // Entrada parcial (bloco sem manifesto completo): não é reutilizada.
    let entry = begin_entry(session_dir, &key).unwrap();
    write_chunk(&entry, 0, FRAMES_100MS);
    assert!(read_cache(session_dir, &key).is_none());

    // A limpeza remove a entrada parcial.
    remove_incomplete(session_dir).unwrap();
    assert!(!session_dir.join("voice-cache").join(&key).exists());

    // Regenerar começa do zero: a pasta nova está vazia e completa normalmente.
    let entry = begin_entry(session_dir, &key).unwrap();
    assert!(!entry.join("chunk-0000.wav").exists());
    write_chunk(&entry, 0, FRAMES_100MS);
    finish_entry(&entry, "text", "af_heart", 10, ENGINE_ID, &[(0, 100)]).unwrap();
    assert!(read_cache(session_dir, &key).is_some());
}

#[test]
fn clear_entry_removes_one_entry_and_a_missing_one_is_ok() {
    let session = tempfile::tempdir().unwrap();
    let session_dir = session.path();
    let key = voice_key("text", "af_heart", 10, ENGINE_ID);
    let entry = begin_entry(session_dir, &key).unwrap();
    write_chunk(&entry, 0, FRAMES_100MS);
    finish_entry(&entry, "text", "af_heart", 10, ENGINE_ID, &[(0, 100)]).unwrap();
    assert!(read_cache(session_dir, &key).is_some());

    clear_entry(session_dir, &key).unwrap();
    assert!(!session_dir.join("voice-cache").join(&key).exists());
    assert!(read_cache(session_dir, &key).is_none());

    // Remover uma entrada ausente é um sucesso.
    clear_entry(session_dir, &key).unwrap();
}

#[test]
fn a_truncated_payload_with_an_intact_header_is_not_reused() {
    let session = tempfile::tempdir().unwrap();
    let key = voice_key("text", "af_heart", 10, ENGINE_ID);
    let entry = begin_entry(session.path(), &key).unwrap();
    let chunk = write_chunk(&entry, 0, FRAMES_100MS);
    finish_entry(&entry, "text", "af_heart", 10, ENGINE_ID, &[(0, 100)]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(chunk)
        .unwrap()
        .set_len(64)
        .unwrap();
    assert!(read_cache(session.path(), &key).is_none());
}

#[test]
fn a_duplicate_index_or_a_false_duration_invalidates_the_manifest() {
    let session = tempfile::tempdir().unwrap();
    let key = voice_key("text", "af_heart", 10, ENGINE_ID);
    let entry = begin_entry(session.path(), &key).unwrap();
    write_chunk(&entry, 0, FRAMES_100MS);
    write_chunk(&entry, 1, FRAMES_100MS);
    finish_entry(
        &entry,
        "text",
        "af_heart",
        10,
        ENGINE_ID,
        &[(0, 100), (1, 100)],
    )
    .unwrap();
    let manifest = entry.join("manifest.json");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    for (field, value) in [("index", 0), ("durationMs", 99)] {
        let mut corrupt = original.clone();
        corrupt["chunks"][1][field] = json!(value);
        std::fs::write(&manifest, corrupt.to_string()).unwrap();
        assert!(read_cache(session.path(), &key).is_none(), "{field}");
    }
}
