//! Voz incremental do tutor: protocolo de blocos, validação da sequência e
//! reprodução em fila com cancelamento.

use std::path::{Path, PathBuf};

use serde_json::json;

use language_assistant_tutor::article::{ChunkRejection, ChunkSequence};
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
