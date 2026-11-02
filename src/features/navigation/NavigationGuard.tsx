/**
 * Decisão partilhada sobre revisões por guardar.
 *
 * A mesma decisão serve a mudança de página, a captura de uma nova tomada e o
 * fecho da janela: o utilizador escolhe guardar, descartar ou cancelar, e a
 * ação pedida só acontece quando a versão mais recente está guardada ou foi
 * explicitamente descartada. Uma escrita falhada ou uma edição feita durante a
 * escrita mantêm o texto, o editor e a janela.
 */

import { useEffect, useRef, useState } from "react";

import type { AppError } from "../../lib/contracts";
import type { SaveOutcome } from "../interview/useInterview";

/** Intenção que originou a decisão pendente. */
export type GuardIntention = "navigate" | "record" | "close";

/** Pedido pendente: a intenção e a ação a executar depois da decisão. */
export interface GuardRequest {
  intention: GuardIntention;
  proceed: () => void | Promise<void>;
}

export interface NavigationGuardProps {
  /** Pedido pendente; nulo quando não há decisão a tomar. */
  request: GuardRequest | null;
  /** Captura ou finalização em curso. */
  busy: boolean;
  /** Existem alterações por guardar. */
  dirty: boolean;
  /** Uma escrita da revisão está em curso. */
  saving: boolean;
  /** Falha da última tentativa de guardar, mantida no diálogo. */
  saveError: AppError | null;
  onSave: () => Promise<SaveOutcome>;
  onDiscard: () => void;
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
};

export default function NavigationGuard({
  request,
  busy,
  dirty,
  saving,
  saveError,
  onSave,
  onDiscard,
  onCancel,
}: NavigationGuardProps) {
  const [running, setRunning] = useState(false);
  const handled = useRef<GuardRequest | null>(null);

  // Sem texto por guardar a decisão não tem nada a resolver: a ação pedida
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

  async function saveAndContinue(): Promise<void> {
    if (!request || busyNow || handled.current === request) {
      return;
    }
    setRunning(true);
    try {
      const outcome = await onSave();
      // `pending` significa que o texto mudou durante a escrita: a versão
      // visível continua por guardar e a decisão não avança.
      if (outcome !== "saved") {
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
    onDiscard();
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
      <h2 id="guard-title">Revisão por guardar</h2>
      <p id="guard-description">
        Existem alterações por guardar na revisão da resposta. Escolha o que
        fazer antes de continuar.
      </p>
      <p className="practice__review-status">
        {saving ? "A guardar revisão…" : "Alterações por guardar"}
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
          {saving ? "A guardar revisão…" : labels.save}
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
