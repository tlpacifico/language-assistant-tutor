/**
 * Decisão partilhada sobre alterações por guardar.
 *
 * A mesma decisão serve a mudança de página, a captura de uma nova tomada, o
 * fecho da janela e a troca de rascunho no editor: o utilizador escolhe guardar,
 * descartar ou cancelar, e a ação pedida só acontece quando todas as versões
 * mais recentes estão guardadas ou foram explicitamente descartadas. Uma
 * escrita falhada ou uma edição feita durante a escrita mantêm o texto, o
 * editor e a janela.
 *
 * Cada editor entrega a sua fonte (`GuardSource`) à aplicação, pelo que a mesma
 * decisão cobre a revisão da resposta e o rascunho do banco de perguntas.
 */

import { useEffect, useRef, useState } from "react";

import type { AppError } from "../../lib/contracts";
import type { SaveOutcome } from "../interview/useInterview";

/** Intenção que originou a decisão pendente. */
export type GuardIntention = "navigate" | "record" | "close" | "switch";

/** Pedido pendente: a intenção e a ação a executar depois da decisão. */
export interface GuardRequest {
  intention: GuardIntention;
  proceed: () => void | Promise<void>;
}

/**
 * Editor com alterações por guardar.
 *
 * `save` devolve `saved` só quando a versão vista ficou persistida; `discard`
 * descarta explicitamente o rascunho. A aplicação reúne todas as fontes e
 * apresenta uma única decisão.
 */
export interface GuardSource {
  key: string;
  /** Área editada, nomeada na mensagem quando há mais do que uma. */
  label: string;
  dirty: boolean;
  saving: boolean;
  error: AppError | null;
  save: () => Promise<SaveOutcome>;
  discard: () => void;
}

export interface NavigationGuardProps {
  /** Pedido pendente; nulo quando não há decisão a tomar. */
  request: GuardRequest | null;
  /** Captura ou finalização em curso. */
  busy: boolean;
  /** Editores com o seu estado atual. */
  sources: GuardSource[];
  onCancel: () => void;
}

/** Rótulos por intenção: cada ação nomeia o que vai acontecer a seguir. */
const LABELS: Record<
  GuardIntention,
  { save: string; discard: string; blocked: string }
> = {
  navigate: {
    save: "Guardar e sair",
    discard: "Descartar e sair",
    blocked:
      "Não é possível mudar de página enquanto a gravação está a ser capturada ou finalizada.",
  },
  record: {
    save: "Guardar e gravar novamente",
    discard: "Descartar e gravar novamente",
    blocked:
      "Não é possível gravar novamente enquanto a gravação está a ser capturada ou finalizada.",
  },
  close: {
    save: "Guardar e fechar",
    discard: "Descartar e fechar",
    blocked:
      "Não é possível fechar enquanto a gravação está a ser capturada ou finalizada.",
  },
  switch: {
    save: "Guardar e continuar",
    discard: "Descartar e continuar",
    blocked:
      "Não é possível trocar de pergunta enquanto a gravação está a ser capturada ou finalizada.",
  },
};

/** Uma fonte isolada, a única revisão, mantém o texto dedicado à resposta. */
function onlyReview(sources: GuardSource[]): boolean {
  const dirty = sources.filter((source) => source.dirty);
  return dirty.length === 1 && dirty[0].key === "review";
}

export default function NavigationGuard({
  request,
  busy,
  sources,
  onCancel,
}: NavigationGuardProps) {
  const [running, setRunning] = useState(false);
  const handled = useRef<GuardRequest | null>(null);

  const dirtySources = sources.filter((source) => source.dirty);
  const dirty = dirtySources.length > 0;
  const saving = sources.some((source) => source.saving);
  const saveError =
    dirtySources.find((source) => source.error !== null)?.error ?? null;

  // Sem alterações por guardar a decisão não tem nada a resolver: a ação pedida
  // segue de imediato, uma única vez por pedido.
  useEffect(() => {
    if (!request || handled.current === request) {
      return;
    }
    if (busy && request.intention !== "close") {
      return;
    }
    if (!dirty) {
      handled.current = request;
      void request.proceed();
    }
  }, [request, busy, dirty]);

  if (!request) {
    return null;
  }

  const labels = LABELS[request.intention];
  const blocked = busy && request.intention !== "close";

  if (blocked) {
    return (
      <div className="practice__notice practice__notice--error" role="alert">
        <p>{labels.blocked}</p>
        <div className="practice__actions">
          <button
            type="button"
            onClick={() => {
              handled.current = request;
              onCancel();
            }}
          >
            Fechar aviso
          </button>
        </div>
      </div>
    );
  }

  if (!dirty) {
    return null;
  }

  const busyNow = running || saving;
  const reviewOnly = onlyReview(sources);

  async function saveAndContinue(): Promise<void> {
    if (!request || busyNow || handled.current === request) {
      return;
    }
    setRunning(true);
    try {
      // Cada editor guarda a sua versão; basta uma escrita não confirmada para
      // a decisão não avançar.
      const outcomes = await Promise.all(
        dirtySources.map((source) => source.save()),
      );
      if (outcomes.some((outcome) => outcome !== "saved")) {
        return;
      }
      handled.current = request;
      await request.proceed();
    } finally {
      setRunning(false);
    }
  }

  function discardAndContinue(): void {
    if (!request || busyNow || handled.current === request) {
      return;
    }
    handled.current = request;
    for (const source of dirtySources) {
      source.discard();
    }
    void request.proceed();
  }

  function cancel(): void {
    if (busyNow) {
      return;
    }
    handled.current = request;
    onCancel();
  }

  return (
    <section
      className="practice__decision"
      role="dialog"
      aria-modal="true"
      aria-labelledby="guard-title"
      aria-describedby="guard-description"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          cancel();
        }
      }}
    >
      <h2 id="guard-title">
        {reviewOnly ? "Revisão por guardar" : "Alterações por guardar"}
      </h2>
      <p id="guard-description">
        {reviewOnly
          ? "Existem alterações por guardar na revisão da resposta. Escolha o que fazer antes de continuar."
          : `Existem alterações por guardar em ${dirtySources
              .map((source) => source.label)
              .join(" e ")}. Escolha o que fazer antes de continuar.`}
      </p>
      <p className="practice__review-status">
        {saving ? "A guardar alterações…" : "Alterações ainda por guardar"}
      </p>
      {saveError ? (
        <div
          className="practice__notice practice__notice--error"
          role="alert"
        >
          <p>{saveError.message}</p>
          <p className="practice__hint">Código: {saveError.code}.</p>
        </div>
      ) : null}
      <div className="practice__actions">
        <button
          type="button"
          autoFocus
          onClick={() => void saveAndContinue()}
          disabled={busyNow}
        >
          {saving ? "A guardar alterações…" : labels.save}
        </button>
        <button
          type="button"
          onClick={discardAndContinue}
          disabled={busyNow}
        >
          {labels.discard}
        </button>
        <button type="button" onClick={cancel} disabled={busyNow}>
          Cancelar
        </button>
      </div>
    </section>
  );
}
