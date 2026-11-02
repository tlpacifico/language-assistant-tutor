//! Contratos e regras puras da fase 2 (entrevistas guiadas).
//!
//! Este módulo não depende de SQLite, de áudio nem do processo auxiliar: reúne
//! os DTOs partilhados com a interface e a validação que o núcleo aplica antes
//! de persistir. `types` espelha `src/lib/interview-contracts.ts`; o exemplo
//! `contracts/interview-v2.example.json` fixa a forma serializada dos dois lados.

mod types;

pub use types::*;
